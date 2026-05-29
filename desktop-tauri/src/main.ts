// CamDroid Desktop GUI — Entry Point
//
// Simple router: connection page → dashboard → back to connection on disconnect.

import "./style.css";
import { renderConnectionPage } from "./pages/connection";
import { renderDashboard, stopStatusPolling } from "./pages/dashboard";

const app = document.getElementById("app")!;

type Page = "connection" | "dashboard";
let currentPage: Page = "connection";

function navigate(page: Page) {
  currentPage = page;

  if (page === "connection") {
    stopStatusPolling();
    renderConnectionPage(app, {
      onConnected: () => navigate("dashboard"),
    });
  } else if (page === "dashboard") {
    renderDashboard(app, {
      onDisconnected: () => navigate("connection"),
    });
  }
}

// Start on the connection page
navigate("connection");
