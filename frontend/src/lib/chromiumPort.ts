// Keep incomplete input as text; validate only when the user saves.
// Zero retains the existing "launch an isolated browser" meaning.
export function parseAttachPort(text: string): number | null {
  if (!/^\d+$/.test(text)) return null
  const port = Number(text)
  return Number.isInteger(port) && port >= 0 && port <= 65535 ? port : null
}
