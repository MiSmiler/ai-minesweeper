// Shared frontend logger (issue #27).
//
// Level policy: dev builds (`vite dev`) log `debug` and above (so the
// gesture state-transition traces are visible); production builds
// (`vite build`) log only `warn`/`error`; tests are silenced.
import { Logger } from "tslog";

const minLevel =
  import.meta.env.MODE === "test" ? 6 : import.meta.env.DEV ? 2 : 4;

export const log = new Logger({ minLevel });
