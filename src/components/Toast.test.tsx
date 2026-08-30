import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Toast, ToastContainer, type ToastData, useToasts } from "./Toast";

describe("Toast Component", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    act(() => {
      vi.runOnlyPendingTimers();
    });
    vi.useRealTimers();
    cleanup();
  });

  describe("Rendering variants", () => {
    it("renders error toast with message, error icon, and error border styling", () => {
      const toast: ToastData = {
        id: "toast-1",
        type: "error",
        message: "Failed to connect to provider",
      };
      const onDismiss = vi.fn();

      const { container } = render(<Toast toast={toast} onDismiss={onDismiss} />);

      expect(screen.getByRole("alert")).toBeTruthy();
      expect(screen.getByText("Failed to connect to provider")).toBeTruthy();
      expect(container.querySelector(".border-status-error")).toBeTruthy();
      expect(container.querySelector(".text-status-error")).toBeTruthy();
      expect(screen.getByRole("alert").getAttribute("data-dismissing")).toBeNull();
    });

    it("renders success toast with message, success icon, and success border styling", () => {
      const toast: ToastData = {
        id: "toast-2",
        type: "success",
        message: "Session saved successfully",
      };
      const onDismiss = vi.fn();

      const { container } = render(<Toast toast={toast} onDismiss={onDismiss} />);

      expect(screen.getByRole("alert")).toBeTruthy();
      expect(screen.getByText("Session saved successfully")).toBeTruthy();
      expect(container.querySelector(".border-status-success")).toBeTruthy();
      expect(container.querySelector(".text-status-success")).toBeTruthy();
    });

    it("renders info toast with message, info icon, and info border styling", () => {
      const toast: ToastData = {
        id: "toast-3",
        type: "info",
        message: "Model update in progress",
      };
      const onDismiss = vi.fn();

      const { container } = render(<Toast toast={toast} onDismiss={onDismiss} />);

      expect(screen.getByRole("alert")).toBeTruthy();
      expect(screen.getByText("Model update in progress")).toBeTruthy();
      expect(container.querySelector(".border-accent-structure")).toBeTruthy();
      expect(container.querySelector(".text-accent-structure")).toBeTruthy();
    });
  });

  describe("Action execution and dismissal triggers", () => {
    it("executes primary action onClick and enters dismissing phase", () => {
      const onClick = vi.fn();
      const toast: ToastData = {
        id: "toast-action",
        type: "error",
        message: "Review failed",
        action: {
          label: "Retry",
          onClick,
        },
      };
      const onDismiss = vi.fn();

      render(<Toast toast={toast} onDismiss={onDismiss} />);

      const retryBtn = screen.getByRole("button", { name: "Retry" });
      expect(retryBtn).toBeTruthy();

      fireEvent.click(retryBtn);

      expect(onClick).toHaveBeenCalledTimes(1);
      const alert = screen.getByRole("alert");
      expect(alert.getAttribute("data-dismissing")).toBe("true");
      expect(onDismiss).not.toHaveBeenCalled();
    });

    it("executes secondary action onClick and enters dismissing phase", () => {
      const primaryClick = vi.fn();
      const secondaryClick = vi.fn();
      const toast: ToastData = {
        id: "toast-secondary",
        type: "error",
        message: "Merge conflict detected",
        action: {
          label: "Resolve",
          onClick: primaryClick,
        },
        secondaryAction: {
          label: "Dismiss Conflict",
          onClick: secondaryClick,
        },
      };
      const onDismiss = vi.fn();

      render(<Toast toast={toast} onDismiss={onDismiss} />);

      const secondaryBtn = screen.getByRole("button", { name: "Dismiss Conflict" });
      fireEvent.click(secondaryBtn);

      expect(secondaryClick).toHaveBeenCalledTimes(1);
      expect(primaryClick).not.toHaveBeenCalled();
      expect(screen.getByRole("alert").getAttribute("data-dismissing")).toBe("true");
      expect(onDismiss).not.toHaveBeenCalled();
    });

    it("triggers exit phase when close button is clicked without double-firing", () => {
      const toast: ToastData = {
        id: "toast-close",
        type: "info",
        message: "New version available",
      };
      const onDismiss = vi.fn();

      render(<Toast toast={toast} onDismiss={onDismiss} />);

      const closeBtn = screen.getByRole("button", { name: "Dismiss notification" });
      fireEvent.click(closeBtn);

      const alert = screen.getByRole("alert");
      expect(alert.getAttribute("data-dismissing")).toBe("true");
      expect(onDismiss).not.toHaveBeenCalled();

      // Clicking again during exit phase does nothing
      fireEvent.click(closeBtn);
      expect(onDismiss).not.toHaveBeenCalled();
    });
  });

  describe("Auto-dismiss timing", () => {
    it("enters dismissing phase after autoDismissMs timer expires", () => {
      const toast: ToastData = {
        id: "toast-timer",
        type: "info",
        message: "Auto disappearing",
        autoDismissMs: 3000,
      };
      const onDismiss = vi.fn();

      render(<Toast toast={toast} onDismiss={onDismiss} />);

      expect(screen.getByRole("alert").getAttribute("data-dismissing")).toBeNull();

      act(() => {
        vi.advanceTimersByTime(2999);
      });
      expect(screen.getByRole("alert").getAttribute("data-dismissing")).toBeNull();
      expect(onDismiss).not.toHaveBeenCalled();

      act(() => {
        vi.advanceTimersByTime(1);
      });
      expect(screen.getByRole("alert").getAttribute("data-dismissing")).toBe("true");
      expect(onDismiss).not.toHaveBeenCalled();
    });

    it("does not auto-dismiss when actions are present", () => {
      const toast: ToastData = {
        id: "toast-action-no-auto",
        type: "error",
        message: "Action required",
        autoDismissMs: 3000,
        action: {
          label: "View",
          onClick: vi.fn(),
        },
      };
      const onDismiss = vi.fn();

      render(<Toast toast={toast} onDismiss={onDismiss} />);

      act(() => {
        vi.advanceTimersByTime(5000);
      });
      expect(screen.getByRole("alert").getAttribute("data-dismissing")).toBeNull();
      expect(onDismiss).not.toHaveBeenCalled();
    });
  });

  describe("Two-phase dismissal and onTransitionEnd / safety timer unmount", () => {
    it("completes unmount when transitionend event fires on the toast element", () => {
      const toast: ToastData = {
        id: "toast-trans",
        type: "info",
        message: "Transition test",
      };
      const onDismiss = vi.fn();

      render(<Toast toast={toast} onDismiss={onDismiss} />);

      const closeBtn = screen.getByRole("button", { name: "Dismiss notification" });
      fireEvent.click(closeBtn);

      const alert = screen.getByRole("alert");
      expect(alert.getAttribute("data-dismissing")).toBe("true");
      expect(onDismiss).not.toHaveBeenCalled();

      // Fire transitionend for opacity
      fireEvent.transitionEnd(alert, { propertyName: "opacity" });
      expect(onDismiss).toHaveBeenCalledTimes(1);
      expect(onDismiss).toHaveBeenCalledWith("toast-trans");

      // Multiple transitionend events (e.g. for transform) do not double-fire onDismiss
      fireEvent.transitionEnd(alert, { propertyName: "transform" });
      expect(onDismiss).toHaveBeenCalledTimes(1);
    });

    it("ignores transitionend events bubbling from child elements", () => {
      const toast: ToastData = {
        id: "toast-child-trans",
        type: "info",
        message: "Child transition test",
      };
      const onDismiss = vi.fn();

      render(<Toast toast={toast} onDismiss={onDismiss} />);

      const closeBtn = screen.getByRole("button", { name: "Dismiss notification" });
      fireEvent.click(closeBtn);

      // Fire transitionend on a child button element
      fireEvent.transitionEnd(closeBtn, { propertyName: "background-color" });
      expect(onDismiss).not.toHaveBeenCalled();
    });

    it("unmounts via the 200ms safety timeout fallback when transitionend does not fire", () => {
      const toast: ToastData = {
        id: "toast-safety",
        type: "success",
        message: "Safety timeout test",
      };
      const onDismiss = vi.fn();

      render(<Toast toast={toast} onDismiss={onDismiss} />);

      const closeBtn = screen.getByRole("button", { name: "Dismiss notification" });
      fireEvent.click(closeBtn);

      expect(screen.getByRole("alert").getAttribute("data-dismissing")).toBe("true");
      expect(onDismiss).not.toHaveBeenCalled();

      act(() => {
        vi.advanceTimersByTime(199);
      });
      expect(onDismiss).not.toHaveBeenCalled();

      act(() => {
        vi.advanceTimersByTime(1);
      });
      expect(onDismiss).toHaveBeenCalledTimes(1);
      expect(onDismiss).toHaveBeenCalledWith("toast-safety");
    });
  });

  describe("prefers-reduced-motion handling", () => {
    it("results in immediate unmount (0ms duration) when prefers-reduced-motion: reduce is set", () => {
      const matchMediaSpy = vi.spyOn(window, "matchMedia").mockImplementation((query) => {
        return {
          matches: query.includes("prefers-reduced-motion: reduce"),
          media: query,
          onchange: null,
          addListener: vi.fn(),
          removeListener: vi.fn(),
          addEventListener: vi.fn(),
          removeEventListener: vi.fn(),
          dispatchEvent: vi.fn(),
        } as unknown as MediaQueryList;
      });

      const toast: ToastData = {
        id: "toast-reduced-motion",
        type: "info",
        message: "Reduced motion test",
      };
      const onDismiss = vi.fn();

      render(<Toast toast={toast} onDismiss={onDismiss} />);

      const closeBtn = screen.getByRole("button", { name: "Dismiss notification" });
      fireEvent.click(closeBtn);

      // With reduced motion, onDismiss is called synchronously without waiting for 150ms/200ms
      expect(onDismiss).toHaveBeenCalledTimes(1);
      expect(onDismiss).toHaveBeenCalledWith("toast-reduced-motion");

      matchMediaSpy.mockRestore();
    });

    it("auto-dismisses immediately when timer expires under reduced motion", () => {
      const matchMediaSpy = vi.spyOn(window, "matchMedia").mockImplementation((query) => {
        return {
          matches: query.includes("prefers-reduced-motion: reduce"),
          media: query,
          onchange: null,
          addListener: vi.fn(),
          removeListener: vi.fn(),
          addEventListener: vi.fn(),
          removeEventListener: vi.fn(),
          dispatchEvent: vi.fn(),
        } as unknown as MediaQueryList;
      });

      const toast: ToastData = {
        id: "toast-reduced-auto",
        type: "info",
        message: "Reduced motion auto dismiss",
        autoDismissMs: 2000,
      };
      const onDismiss = vi.fn();

      render(<Toast toast={toast} onDismiss={onDismiss} />);

      act(() => {
        vi.advanceTimersByTime(2000);
      });

      expect(onDismiss).toHaveBeenCalledTimes(1);
      expect(onDismiss).toHaveBeenCalledWith("toast-reduced-auto");

      matchMediaSpy.mockRestore();
    });
  });

  describe("useToasts and ToastContainer", () => {
    it("renders nothing when toasts array is empty", () => {
      const onDismiss = vi.fn();
      const { container } = render(<ToastContainer toasts={[]} onDismiss={onDismiss} />);
      expect(container.firstChild).toBeNull();
    });

    it("renders container with multiple toasts", () => {
      const toasts: ToastData[] = [
        { id: "1", type: "error", message: "Error msg" },
        { id: "2", type: "success", message: "Success msg" },
      ];
      const onDismiss = vi.fn();

      render(<ToastContainer toasts={toasts} onDismiss={onDismiss} />);

      expect(screen.getByText("Error msg")).toBeTruthy();
      expect(screen.getByText("Success msg")).toBeTruthy();
    });

    it("adds toasts correctly using useToasts hook helpers", () => {
      function TestHost() {
        const { toasts, showError, showSuccess, showInfo, dismissToast } = useToasts();
        return (
          <div>
            <button type="button" onClick={() => showError("Err")}>
              Add Error
            </button>
            <button type="button" onClick={() => showSuccess("Succ")}>
              Add Success
            </button>
            <button type="button" onClick={() => showInfo("Inf")}>
              Add Info
            </button>
            <button
              type="button"
              onClick={() => {
                showError("Err with actions", {
                  primary: { label: "P", onClick: () => {} },
                  secondary: { label: "S", onClick: () => {} },
                });
              }}
            >
              Add Error With Multi Actions
            </button>
            <ToastContainer toasts={toasts} onDismiss={dismissToast} />
          </div>
        );
      }

      render(<TestHost />);

      fireEvent.click(screen.getByText("Add Error"));
      expect(screen.getByText("Err")).toBeTruthy();

      fireEvent.click(screen.getByText("Add Success"));
      expect(screen.getByText("Succ")).toBeTruthy();

      fireEvent.click(screen.getByText("Add Info"));
      expect(screen.getByText("Inf")).toBeTruthy();

      fireEvent.click(screen.getByText("Add Error With Multi Actions"));
      expect(screen.getByText("Err with actions")).toBeTruthy();
      expect(screen.getByText("P")).toBeTruthy();
      expect(screen.getByText("S")).toBeTruthy();
    });
  });
});
