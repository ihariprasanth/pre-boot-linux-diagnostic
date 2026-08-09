import { Link } from "react-router-dom";
import { BootLine } from "../components/BootLine";

export function NotFoundPage() {
  return (
    <>
      <BootLine>route not found\u2026</BootLine>
      <h1 className="page-title">404</h1>
      <p className="page-sub">
        Nothing here. <Link to="/">Back to the fleet.</Link>
      </p>
    </>
  );
}
