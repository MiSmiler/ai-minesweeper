// Shared frontend logger (issue #27).
//
// Level policy: dev builds (`vite dev`) log `info` and above; production
// builds (`vite build`) log only `warn`/`error`; tests are silenced.
import { Logger } from "tslog";

const minLevel =
  import.meta.env.MODE === "test" ? 6 : import.meta.env.DEV ? 3 : 4;

export const log = new Logger({ minLevel });
