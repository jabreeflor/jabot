//! Turning a file the user picked into something a bot row can hold.
//!
//! A bot's icon is a colour and its initials until someone gives it a picture,
//! and the picture has to survive a restart — so it travels the same way every
//! other field of a bot does: as a string on the `bots` row, through
//! `crew/update`, back out of `crew/list`. A path would not: the file the user
//! picked is theirs, it can be renamed or unplugged, and an avatar that turns
//! into a broken image because a volume was ejected is worse than no avatar.
//!
//! So the file is normalised here, in the renderer, before it is ever sent:
//! centre-cropped to a square, scaled down to [`AVATAR_BOX`], re-encoded, and
//! carried as a `data:` URL. That is what keeps a 12-megapixel photo from
//! becoming a 12-megapixel row, and it is why the host's own cap (which it
//! enforces again, because a host does not trust its caller) is a number this
//! side can nearly always meet.

/** The square every uploaded icon is reduced to, in device-independent pixels.
    Twice the largest place an avatar is drawn (54px), so it still has pixels
    to spare on a retina panel and none to waste anywhere else. */
export const AVATAR_BOX = 256;

/** The most an encoded icon may weigh. Matches the host's cap exactly — a
    renderer that sent something the host would refuse would be reporting the
    refusal to a user who has no way to act on it. */
export const MAX_IMAGE_BYTES = 256 * 1024;

/** What a stored `data:` URL may claim to be — exactly the three the encoder
    below can produce. Closed, and closed on purpose: an `image/svg+xml` icon
    is a document with script in it, and this one is rendered from a row that
    round-trips through the host. A GIF the user picks is fine as *input*; it
    arrives here as pixels and leaves as one of these three. */
const IMAGE_TYPES = ["png", "jpeg", "webp"] as const;

const IMAGE_URL = new RegExp(
  `^data:image/(?:${IMAGE_TYPES.join("|")});base64,[A-Za-z0-9+/]+={0,2}$`,
);

/**
 * Is this a stored icon, rather than something else that reached the field?
 *
 * The renderer checks what the host sends it for the same reason the host
 * checks what the renderer sends: this string ends up in a `src`, and the one
 * shape that must never get there is a URL that fetches — `http:` for the
 * tracking pixel, `svg+xml` for the script.
 */
export function isBotImage(value: string): boolean {
  return IMAGE_URL.test(value);
}

/**
 * What the encoded icon actually weighs, from the base64 alone.
 *
 * Every four characters carry three bytes, less one per `=` of padding. Cheap
 * enough to call on every candidate encoding, which is what the loop below
 * needs — `Blob` would mean building one just to ask its size.
 */
export function imageBytes(dataUrl: string): number {
  const base64 = dataUrl.slice(dataUrl.indexOf(",") + 1);
  const padding = base64.endsWith("==") ? 2 : base64.endsWith("=") ? 1 : 0;
  return Math.floor((base64.length * 3) / 4) - padding;
}

/**
 * The largest centred square inside a `width`×`height` image.
 *
 * Cropping rather than letterboxing because the frame is a circle: a portrait
 * fitted whole into a round hole is a small picture with two bald patches,
 * where the centre crop is the part of the photo the person aimed at.
 */
export function coverCrop(
  width: number,
  height: number,
): { x: number; y: number; size: number } {
  const size = Math.min(width, height);
  return { x: (width - size) / 2, y: (height - size) / 2, size };
}

/**
 * The encodings tried, in the order they are tried.
 *
 * WebP first because it is the only one of the three that is both small and
 * able to keep transparency, which a logo needs and a photo does not care
 * about. PNG next so a transparent icon that WebP could not fit still keeps
 * its alpha if it is simple enough to fit losslessly. JPEG last: it is the
 * one that always gets small, and the one that fills the transparent corners
 * with black to do it.
 *
 * An engine that does not know a type does not say so — `toDataURL` quietly
 * answers in PNG — so each result is checked against what was asked for, and
 * a lie is skipped rather than shipped under the wrong name.
 */
const ENCODINGS: readonly { type: string; quality: number }[] = [
  { type: "image/webp", quality: 0.9 },
  { type: "image/webp", quality: 0.75 },
  { type: "image/png", quality: 1 },
  { type: "image/jpeg", quality: 0.85 },
  { type: "image/jpeg", quality: 0.6 },
];

/** What the editor shows when a file cannot become an icon. Thrown rather
    than returned as null so the reason survives to the user. */
export class ImageError extends Error {}

/**
 * A file from the picker, as a stored icon.
 *
 * Rejects with an [`ImageError`] whose message is meant to be shown: every
 * failure here is something the user chose and can choose differently.
 */
export async function readBotImage(file: File): Promise<string> {
  if (!file.type.startsWith("image/")) {
    throw new ImageError(`${file.name} is not an image`);
  }
  const image = await decodeImage(file);
  return encodeSquare(image);
}

/** The file as a decoded bitmap. `FileReader` and not `URL.createObjectURL`
    so there is no object URL to leak if the decode never finishes. */
function decodeImage(file: File): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onerror = () =>
      reject(new ImageError(`Could not read ${file.name}`));
    reader.onload = () => {
      const image = new Image();
      image.onload = () => resolve(image);
      image.onerror = () =>
        reject(
          new ImageError(`${file.name} is not an image this Mac can read`),
        );
      image.src = String(reader.result);
    };
    reader.readAsDataURL(file);
  });
}

/** Centre-crop, scale to the box, and encode small enough to store. */
function encodeSquare(image: HTMLImageElement): string {
  const width = image.naturalWidth || image.width;
  const height = image.naturalHeight || image.height;
  if (width === 0 || height === 0) {
    throw new ImageError("That image has no pixels in it");
  }
  const crop = coverCrop(width, height);
  const canvas = document.createElement("canvas");
  // Never scaled *up*: a 40px favicon blown up to 256 is four times the bytes
  // for the same blur, and the avatar box will scale it on screen anyway.
  canvas.width = Math.min(AVATAR_BOX, crop.size);
  canvas.height = canvas.width;
  const context = canvas.getContext("2d");
  if (!context) throw new ImageError("This Mac cannot resize images");
  context.drawImage(
    image,
    crop.x,
    crop.y,
    crop.size,
    crop.size,
    0,
    0,
    canvas.width,
    canvas.height,
  );

  for (const { type, quality } of ENCODINGS) {
    const url = canvas.toDataURL(type, quality);
    if (!url.startsWith(`data:${type};base64,`)) continue;
    if (imageBytes(url) <= MAX_IMAGE_BYTES) return url;
  }
  throw new ImageError("That image is too detailed to store as an icon");
}
