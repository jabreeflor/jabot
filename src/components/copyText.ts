//! Copy a string to the clipboard.
//!
//! `navigator.clipboard.writeText` is the path a secure context offers. The
//! textarea fallback is for the cases it does not — an older webview, a
//! denied permission — so a failed write is a real refusal, not a missing
//! API. Callers surface that refusal; this module does not.

export async function copyText(text: string): Promise<void> {
  if (navigator.clipboard?.writeText) {
    try {
      await navigator.clipboard.writeText(text);
      return;
    } catch {
      // Fall through: a denied permission still has execCommand on some hosts.
    }
  }
  fallbackCopy(text);
}

function fallbackCopy(text: string): void {
  if (typeof document.execCommand !== "function") {
    throw new Error("Couldn't copy to the clipboard");
  }
  const field = document.createElement("textarea");
  field.value = text;
  field.setAttribute("readonly", "");
  field.style.position = "fixed";
  field.style.left = "-9999px";
  document.body.appendChild(field);
  field.select();
  let ok = false;
  try {
    ok = document.execCommand("copy");
  } finally {
    field.remove();
  }
  if (!ok) {
    throw new Error("Couldn't copy to the clipboard");
  }
}
