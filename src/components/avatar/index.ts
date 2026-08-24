//! The module's front door.
//!
//! Call sites import from here and never from a renderer directly — that is
//! what keeps "which crew is on screen" a single setting rather than a
//! decision six views each get to make.

export { Avatar, CrewAvatar } from "./Avatar";
export {
  CrewStyleProvider,
  useCrewStyle,
  useSetCrewStyle,
  useCrewStyleChoice,
  loadCrewStyle,
  saveCrewStyle,
} from "./CrewStyleContext";
export {
  CREW_STYLES,
  CREW_STYLE_KEY,
  DEFAULT_CREW_STYLE,
  avatarStateFor,
} from "./crew";
export type { AvatarState, CrewRenderProps, CrewStyle } from "./crew";
