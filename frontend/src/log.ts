// Shared frontend logger (issue #27).
//
// Level policy: dev builds (`vite dev`) log `debug` and above (so the
// gesture state-change and action traces are visible); production builds
// (`vite build`) log only `warn`/`error`; tests are silenced. A build-time
// `VITE_LOG_LEVEL` override (a level name, e.g. `VITE_LOG_LEVEL=debug npm
// run build`) re-gates any non-test build.
import { Logger } from "tslog";

/** tslog level numbers keyed by name (silly=0 … fatal=6). */
const LEVELS: Record<string, number> = {
  silly: 0,
  trace: 1,
  debug: 2,
  info: 3,
  warn: 4,
  error: 5,
  fatal: 6,
};

const defaultMinLevel =
  import.meta.env.MODE === "test" ? 6 : import.meta.env.DEV ? 2 : 4;

const override =
  import.meta.env.VITE_LOG_LEVEL &&
  LEVELS[import.meta.env.VITE_LOG_LEVEL.toLowerCase()];

// Tests stay silenced even when an override is set (the suite asserts
// behavior, not log output).
const minLevel =
  import.meta.env.MODE === "test" ? 6 : (override ?? defaultMinLevel);

export const log = new Logger({ minLevel });
