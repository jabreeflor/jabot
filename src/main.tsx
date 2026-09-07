import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import { bootTheme } from "./theme";
import { bootTranslucency } from "./translucency";

const theme = bootTheme();
bootTranslucency(theme);

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
