// Browser-only visual fixture. No native capture, files, registry or installer is invoked.
import { createRoot } from "react-dom/client";
import { App } from "../../ui/src/App";
import { loadLocale } from "../../ui/src/lib/i18n";
import "./style.css";
await loadLocale();
createRoot(document.getElementById("root")!).render(<App />);
