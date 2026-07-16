export interface FloatingBarTerminalPresentation {
  epoch: number;
  isCurrent: (epoch: number) => boolean;
  resize: () => Promise<void>;
  show: () => Promise<void>;
  scheduleHide: () => void;
}

export async function runFloatingBarTerminalPresentation(
  presentation: FloatingBarTerminalPresentation
): Promise<boolean> {
  await presentation.resize();
  if (!presentation.isCurrent(presentation.epoch)) return false;
  await presentation.show();
  if (!presentation.isCurrent(presentation.epoch)) return false;
  presentation.scheduleHide();
  return true;
}
