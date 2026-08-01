import type { Attachment } from "../types";

/**
 * Composer image attachments: browser `File`/`Blob` → inline-base64
 * `Attachment` ready for the agent.
 *
 * Attachments are stored inline in the conversation transcript rather than as
 * files on disk, so payload size is a first-class concern here — an untouched
 * 4K screenshot is ~6MB of base64 per turn. Everything in this module exists
 * to keep that number small before it reaches the backend.
 */

/**
 * Longest edge, in pixels, an image is downscaled to before encoding.
 *
 * 1568px is the largest dimension Anthropic's vision models actually use —
 * anything bigger is resized server-side anyway, so the extra bytes buy no
 * accuracy and cost transcript size on every load. Images already smaller than
 * this are passed through untouched (upscaling would only add bytes).
 */
const MAX_EDGE_PX = 1568;

/** Media types the backend accepts. Anything else is rejected up front. */
const ACCEPTED = ["image/png", "image/jpeg", "image/gif", "image/webp"];

/**
 * Mirrors the backend's per-image budget (`limits::ATTACHMENT_MAX_BYTES`).
 * Duplicated rather than shared because the check must happen here to give
 * the user an immediate, specific message instead of a failed turn.
 */
const MAX_BASE64_BYTES = 4 * 1024 * 1024;

/** Mirrors `limits::ATTACHMENTS_MAX_COUNT`. */
export const MAX_ATTACHMENTS = 8;

/** A rejected file, with a reason worth showing the user. */
export class AttachmentError extends Error {}

/**
 * Whether a `File`/`Blob` looks like an image this composer can attach.
 *
 * Used to filter paste and drop payloads, which routinely carry non-image
 * items (text/html flavours of a copied region, a dragged `.zip`) that should
 * be ignored silently rather than raised as errors.
 */
export function isAttachableImage(file: Blob): boolean {
  return ACCEPTED.includes(file.type);
}

/**
 * Convert an image file into a downscaled, base64-encoded `Attachment`.
 *
 * GIFs bypass the canvas path: drawing one to a canvas keeps only the first
 * frame, silently destroying the animation that was probably the reason for
 * attaching it. They're passed through at original size instead, where the
 * size guard still applies.
 *
 * Re-encodes to PNG for lossless sources and JPEG for photographic ones,
 * preserving whichever the source was, so a screenshot's text stays crisp
 * while a photo doesn't balloon.
 *
 * @throws {AttachmentError} if the type is unsupported, the bytes don't decode
 *   as an image, or the result still exceeds the size budget after downscaling.
 */
export async function fileToAttachment(file: Blob): Promise<Attachment> {
  if (!isAttachableImage(file)) {
    throw new AttachmentError(
      `Unsupported image type${file.type ? `: ${file.type}` : ""}. Use PNG, JPEG, GIF, or WebP.`,
    );
  }

  const attachment =
    file.type === "image/gif"
      ? { media_type: file.type, data: await blobToBase64(file) }
      : await downscaleToAttachment(file);

  if (attachment.data.length > MAX_BASE64_BYTES) {
    throw new AttachmentError(
      "Image is too large to attach even after downscaling. Try cropping it first.",
    );
  }
  return attachment;
}

/** Decode, downscale if oversized, and re-encode. */
async function downscaleToAttachment(file: Blob): Promise<Attachment> {
  const bitmap = await decode(file);
  try {
    const scale = Math.min(
      1,
      MAX_EDGE_PX / Math.max(bitmap.width, bitmap.height),
    );
    // Already within budget — skip the re-encode entirely and keep the
    // original bytes, which avoids a needless generational quality loss on
    // JPEGs and is faster for the common small-screenshot case.
    if (scale === 1) {
      return { media_type: file.type, data: await blobToBase64(file) };
    }

    const canvas = document.createElement("canvas");
    canvas.width = Math.round(bitmap.width * scale);
    canvas.height = Math.round(bitmap.height * scale);
    const ctx = canvas.getContext("2d");
    if (!ctx) {
      throw new AttachmentError("Could not process the image in this window.");
    }
    ctx.drawImage(bitmap, 0, 0, canvas.width, canvas.height);

    // Keep JPEG as JPEG (photos compress far better lossy); everything else
    // becomes PNG so screenshots and diagrams stay sharp.
    const outType = file.type === "image/jpeg" ? "image/jpeg" : "image/png";
    const blob = await new Promise<Blob | null>((resolve) =>
      canvas.toBlob(resolve, outType, 0.9),
    );
    if (!blob) {
      throw new AttachmentError("Could not encode the resized image.");
    }
    return { media_type: outType, data: await blobToBase64(blob) };
  } finally {
    bitmap.close();
  }
}

/** `createImageBitmap` with a friendlier failure for corrupt/partial data. */
async function decode(file: Blob): Promise<ImageBitmap> {
  try {
    return await createImageBitmap(file);
  } catch {
    throw new AttachmentError("That file could not be read as an image.");
  }
}

/**
 * Base64 of a blob's bytes, with the `data:<type>;base64,` prefix stripped —
 * the backend rejects a payload that still carries it.
 *
 * Uses FileReader rather than `btoa` over a byte string: `btoa` throws on
 * values above U+00FF and needs a manual chunked loop to avoid blowing the
 * argument limit on multi-MB images.
 */
function blobToBase64(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onerror = () =>
      reject(new AttachmentError("Could not read the image data."));
    reader.onload = () => {
      const result = String(reader.result);
      const comma = result.indexOf(",");
      resolve(comma === -1 ? result : result.slice(comma + 1));
    };
    reader.readAsDataURL(blob);
  });
}

/**
 * Inverse of {@link blobToBase64}, for bytes that arrived already-encoded.
 *
 * Used by the drag-and-drop path: the backend reads the dropped file and
 * returns base64 at original size, and this turns it back into a `Blob` so it
 * can go through the same downscale pipeline as a pasted image instead of
 * bypassing it.
 */
export function base64ToBlob(data: string, mediaType: string): Promise<Blob> {
  const binary = atob(data);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return Promise.resolve(new Blob([bytes], { type: mediaType }));
}

/** Build a displayable `data:` URI for rendering an attachment thumbnail. */
export function attachmentSrc(attachment: Attachment): string {
  return `data:${attachment.media_type};base64,${attachment.data}`;
}
