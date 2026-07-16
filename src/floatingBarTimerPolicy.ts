export type FloatingBarTimerBoundary =
  | "new_session_event"
  | "component_unmount"
  | "listener_reregister";

export function shouldClearFloatingBarTerminalTimer(boundary: FloatingBarTimerBoundary): boolean {
  return boundary !== "listener_reregister";
}
