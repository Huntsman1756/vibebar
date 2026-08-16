export const POPOVER_OPENED_EVENT = "vibebar://popover-opened";

export function shouldAutoRefreshOnMount(isPopover: boolean): boolean {
  return !isPopover;
}
