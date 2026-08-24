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
/** The deal, for the two call sites that are not drawing a bot: the shell
    seeds the roster it holds, and an avatar that stands for something other
    than a crew member pins itself to a place instead of taking one. */
export { reserveDeal, seedDealOrder } from "./hash";
export {
  CREW_STYLES,
  CREW_STYLE_KEY,
  DEFAULT_CREW_STYLE,
  avatarStateFor,
} from "./crew";
export type { AvatarState, CrewRenderProps, CrewStyle } from "./crew";
