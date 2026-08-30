//! The module's front door.

export { Avatar, CrewAvatar } from "./Avatar";
export { avatarStateFor } from "./state";
export type { AvatarState } from "./state";
/** The upload path, for the one screen that offers it. */
export {
  AVATAR_BOX,
  ImageError,
  MAX_IMAGE_BYTES,
  isBotImage,
  readBotImage,
} from "./image";
export { monogram } from "./monogram";
