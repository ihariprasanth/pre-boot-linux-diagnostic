import { NavLink, Outlet } from "react-router-dom";
import { HealthIndicator } from "./HealthIndicator";

export function Layout() {
  return (
    <div className="shell">
      <header className="topbar">
        <span className="brand">
          PLDDS <span className="brand-dim">/ fleet console</span>
        </span>
        <nav>
          <NavLink to="/" end className={({ isActive }) => (isActive ? "active" : "")}>
            devices
          </NavLink>
        </nav>
        <span className="topbar-spacer" />
        <HealthIndicator />
      </header>
      <main className="main">
        <Outlet />
      </main>
    </div>
  );
}
