import "@typespec/http";
import "@typespec/openapi";
import "@typespec/openapi3";
import "@typespec/versioning";

using Http;
using OpenAPI;
using Versioning;

/**
 * A Nix binary cache on Cloudflare Workers and R2. It has no authentication
 * or authorization: anyone who can reach the Worker can read and upload.
 * Protect it with Cloudflare Access or a gateway of your own.
 *
 * Errors are JSON, `{ error, message }`. A request that doesn't fit
 * this document (a body over the size limit, the wrong content type) is
 * rejected before it reaches the Worker, as `application/problem+json`.
 */
@service(#{ title: "nix-cloudflare" })
@info(#{ license: #{ name: "MIT" } })
@tagMetadata("CacheService", #{ description: "The Nix binary cache protocol." })
@versioned(Versions)
namespace Nix.Cache.v1;

enum Versions {
  v1,
}

/** An error, `{ error, message }`. */
model Error {
  error: ErrorCode;

  /** What went wrong. Nix prints it when an upload fails. */
  message: string;
}

enum ErrorCode {
  bad_request,
  not_found,
  misconfigured,
  internal_error,
}

// A response with no body, described by `Description`.
@doc(Description)
model Empty<Code, Description extends valueof string> {
  @statusCode _: Code;
}

/** The narinfo or request is invalid. */
@error
model BadRequest {
  @statusCode _: 400;
  @body body: Error;
}

/** No such object. */
@error
model NotFound {
  @statusCode _: 404;
  @body body: Error;
}

/**
 * The bindings or the signing key are invalid
 * (`misconfigured`), or a stored object is unreadable (`internal_error`).
 * Details go only to the logs.
 */
@error
model InternalServerError {
  @statusCode _: 500;
  @body body: Error;
}

/** What an upload can fail with. */
alias UploadErrors = InternalServerError;

@tag("CacheService")
interface CacheService {
  /**
   * The store directory, priority and mass-query support, in the format Nix reads,
   * and a `PublicKey:` line with the cache's signing key, `<key-name>:<base64>`,
   * when it has one. Nix ignores that line: add the key to `trusted-public-keys`.
   *
   * @returns The cache's metadata.
   */
  @summary("Cache metadata")
  @operationId("getNixCacheInfo")
  @route("/nix-cache-info")
  @get
  getNixCacheInfo(): {
    @header contentType: "text/x-nix-cache-info";
    @body body: string;
  };

  /**
   * Takes a newline-separated list of store path hashes and returns the
   * ones that are in the cache, so a client can check many at once.
   *
   * @returns The cached subset of the hashes, one per line.
   */
  @summary("Mass query")
  @operationId("postMassQuery")
  @route("/")
  @post
  postMassQuery(@header contentType: "text/plain", @body body: string):
    | {
        @header contentType: "text/plain";
        @body body: string;
      }
    | InternalServerError;

  /** Lets an uploader skip store paths the cache already has. */
  @summary("Check for a narinfo")
  @operationId("headNarInfo")
  @route("/{hash}.narinfo")
  @head
  headNarInfo(
    /** The hash part of a store path. */
    @path hash: string,
  ):
    | Empty<200, "The narinfo exists.">
    | Empty<404, "No such narinfo.">
    | Empty<500, "The bucket couldn't be read.">;

  /**
   * The narinfo for a store path, as it was uploaded, with a `Sig:` line if
   * the Worker signed it.
   *
   * @returns The narinfo.
   */
  @summary("Get a narinfo")
  @operationId("getNarInfo")
  @route("/{hash}.narinfo")
  @get
  getNarInfo(
    /** The hash part of a store path. */
    @path hash: string,
  ):
    | {
        @header contentType: "text/x-nix-narinfo";
        @body body: string;
      }
    | NotFound
    | InternalServerError;

  /**
   * Parses and validates the narinfo, and requires its `StorePath` to match
   * the hash. It is stored as sent. If it has no `Sig:`, the Worker signs it
   * with `NIX_CLOUDFLARE_API_SECRET`, and rejects it when no key is set.
   */
  @summary("Upload a narinfo")
  @operationId("putNarInfo")
  @route("/{hash}.narinfo")
  @put
  putNarInfo(
    /** The hash part of a store path. */
    @path hash: string,

    @header contentType: "text/x-nix-narinfo",
    @body body: string,
  ): Empty<200, "The narinfo was stored."> | BadRequest | UploadErrors;

  /** Lets an uploader skip NARs the cache already has. */
  @summary("Check for a NAR")
  @operationId("headNar")
  @route("/nar/{hash}.nar")
  @head
  headNar(
    /** The NAR's file hash, from the narinfo's `URL`. */
    @path hash: string,
  ):
    | Empty<200, "The NAR exists.">
    | Empty<404, "No such NAR.">
    | Empty<500, "The bucket couldn't be read.">;

  /**
   * The NAR archive a narinfo's `URL` points at.
   *
   * @returns The NAR.
   */
  @summary("Get a NAR")
  @operationId("getNar")
  @route("/nar/{hash}.nar")
  @get
  getNar(
    /** The NAR's file hash, from the narinfo's `URL`. */
    @path hash: string,
  ):
    | {
        @header contentType: "application/x-nix-archive";
        @body body: bytes;
      }
    | NotFound
    | InternalServerError;

  /**
   * Stores a NAR archive. Compressed NARs aren't supported: upload with
   * `?compression=none`.
   */
  @summary("Upload a NAR")
  @operationId("putNar")
  @route("/nar/{hash}.nar")
  @put
  putNar(
    /** The NAR's file hash, from the narinfo's `URL`. */
    @path hash: string,

    // What Nix sends; it's served back as application/x-nix-archive.
    @header contentType: "application/x-nix-nar",

    @body body: bytes,
  ): Empty<200, "The NAR was stored."> | UploadErrors;
}
