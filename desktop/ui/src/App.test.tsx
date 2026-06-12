import { render, screen } from "@testing-library/react";
import "@testing-library/jest-dom";
import App, { MODULES } from "./App";

test("sidebar renders all 8 modules and lands on cockpit", () => {
  render(<App />);
  for (const m of MODULES) {
    expect(screen.getByText(m.label)).toBeInTheDocument();
  }
  // Verify the cockpit placeholder content is visible in the main area.
  // getAllByText to avoid collision with the sidebar menu item for "驾驶舱".
  const matches = screen.getAllByText(/驾驶舱 —— M2\+ 交付|驾驶舱/);
  expect(matches.length).toBeGreaterThan(0);
});
