import ReactDOM from "react-dom/client";
import App from "./App";

// The webview's own context menu (Back, Reload, Inspect...) is never shown;
// the app opens its own menus where it has something to offer.
document.addEventListener("contextmenu", (e) => e.preventDefault());

// No StrictMode: its double-mounted effects would attach terminals twice.
ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(<App />);
