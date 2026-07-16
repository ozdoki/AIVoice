export type SelectedVoiceEditMutation = "replace" | "cancel" | null;

export function shouldFinalizeSelectedVoiceEditExpiry(
  expiryPending: boolean,
  mutation: SelectedVoiceEditMutation,
  completed: boolean,
  expiryFinalizing: boolean
): boolean {
  return expiryPending && mutation === null && !completed && !expiryFinalizing;
}
