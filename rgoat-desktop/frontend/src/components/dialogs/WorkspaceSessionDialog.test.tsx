import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import WorkspaceSessionDialog from "./WorkspaceSessionDialog";

describe("WorkspaceSessionDialog", () => {
  it("creates in the temporary workspace when selected", async () => {
    const onUseTemporary = vi.fn().mockResolvedValue(undefined);
    render(
      <WorkspaceSessionDialog
        isOpen
        currentWorkspace={null}
        onUseTemporary={onUseTemporary}
        onChooseDirectory={vi.fn()}
        onClose={vi.fn()}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "使用临时工作空间" }));
    expect(onUseTemporary).toHaveBeenCalledTimes(1);
  });
});
