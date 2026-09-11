import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import { bootTheme } from "./theme";
import { bootTranslucency } from "./translucency";
import { bootWindowChrome } from "./windowChrome";

bootTheme();
void bootWindowChrome();
void bootTranslucency();

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
