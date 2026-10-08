/** Round upwards so a future press is never presented as 0.0s. No UI timer. */
export function autoplayCountdown(ms: number): string {
  return `${(Math.ceil(Math.max(1, ms) / 100) / 10).toFixed(1)}s`
}
