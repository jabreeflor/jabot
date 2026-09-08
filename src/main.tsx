import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import { bootTheme } from "./theme";
import { bootTranslucency } from "./translucency";

bootTheme();
void bootTranslucency();

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
