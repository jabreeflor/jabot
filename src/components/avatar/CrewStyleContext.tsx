//! Which crew is on screen, and where that choice lives.
//!
//! A context and not a prop: the avatar appears in the sidebar, the chat
//! header, the Inbox, the crew page and the bot editor, and threading a style
//! through all five would make every intermediate component care about a
//! setting none of them has an opinion about.
//!
//! The default is deliberately reachable without a provider. Tests render an
//! Inbox row or a bot tile in isolation, and a context that throws outside its
//! provider would turn "this component renders" into "this component renders
//! inside a tree it does not otherwise need". Reading the setting is
//! therefore always safe; only *changing* it needs the provider, and the one
//! screen that changes it is the one that mounts it.

import {
  createContext,
  useCallback,
  useContext,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import {
  CREW_STYLE_KEY,
  DEFAULT_CREW_STYLE,
  CREW_STYLES,
  type CrewStyle,
} from "./crew";

interface CrewStyleValue {
  style: CrewStyle;
  setStyle: (style: CrewStyle) => void;
}

/**
 * Read outside a provider and you get the default and a setter that does
 * nothing. That is the honest shape: there is nowhere to put the value, so
 * pretending the write happened would leave the caller waiting for a
 * re-render that is never coming.
 */
const CrewStyleContext = createContext<CrewStyleValue>({
  style: DEFAULT_CREW_STYLE,
  setStyle: () => {},
});

function isCrewStyle(value: unknown): value is CrewStyle {
  return CREW_STYLES.some((entry) => entry.id === value);
}

/**
 * Pure and side-effect free, because it is used as a lazy `useState`
 * initializer and StrictMode calls those twice. A store that will not answer
 * — a private window, a webview with site data off — is not an error worth
 * surfacing: the app has a perfectly good default and the only cost is that
 * the choice does not survive a restart.
 */
export function loadCrewStyle(): CrewStyle {
  let raw: string | null;
  try {
    raw = window.localStorage.getItem(CREW_STYLE_KEY);
  } catch {
    return DEFAULT_CREW_STYLE;
  }
  return isCrewStyle(raw) ? raw : DEFAULT_CREW_STYLE;
}

export function saveCrewStyle(style: CrewStyle): void {
  try {
    window.localStorage.setItem(CREW_STYLE_KEY, style);
  } catch {
    // The switch still works for this session; it just forgets on restart.
  }
}

export function CrewStyleProvider({ children }: { children: ReactNode }) {
  const [style, setStyleState] = useState<CrewStyle>(loadCrewStyle);

  const setStyle = useCallback((next: CrewStyle) => {
    setStyleState(next);
    saveCrewStyle(next);
  }, []);

  // The value is memoised because it sits above the whole tree: a fresh
  // object every render would re-render every avatar on every parent update,
  // which is the one thing this context must not cost.
  const value = useMemo(() => ({ style, setStyle }), [style, setStyle]);

  return (
    <CrewStyleContext.Provider value={value}>
      {children}
    </CrewStyleContext.Provider>
  );
}

/** What to draw. The hook every avatar calls. */
export function useCrewStyle(): CrewStyle {
  return useContext(CrewStyleContext).style;
}

/** The setter, for the one screen that offers the choice. */
export function useSetCrewStyle(): (style: CrewStyle) => void {
  return useContext(CrewStyleContext).setStyle;
}

/** Both halves, for a control that shows the current value and changes it. */
export function useCrewStyleChoice(): CrewStyleValue {
  return useContext(CrewStyleContext);
}
