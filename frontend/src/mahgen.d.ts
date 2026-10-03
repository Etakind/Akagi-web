// mahgen 1.0.0 publishes declarations but omits them from its exports map.
// Describe only the public entry point used by our renderer.
declare module 'mahgen' {
  export class Mahgen {
    static render(sequence: string, riverMode?: boolean): Promise<string>
  }
}
