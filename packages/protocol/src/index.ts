import { z } from 'zod';

export const PROTOCOL_VERSION = 1;

export const ErrorCode = z.enum([
  'offline',
  'disabled',
  'not_permitted',
  'needs_approval',
  'invalid_params',
  'module_failed',
  'timeout',
  'busy',
  'unauthorized',
  'bad_request',
  'internal',
]);
export type ErrorCode = z.infer<typeof ErrorCode>;

export const ErrorInfo = z.object({
  code: ErrorCode,
  message: z.string(),
});

export const AiTier = z.enum(['safe', 'confirm', 'never']);

export const ParamSpec = z.object({
  type: z.enum(['int', 'float', 'string', 'bool', 'enum']),
  description: z.string().optional(),
  default: z.unknown().optional(),
  min: z.number().optional(),
  max: z.number().optional(),
  options: z.array(z.string()).optional(),
});

export const ActionSpec = z.object({
  id: z.string().regex(/^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)+$/),
  label: z.string(),
  icon: z.string().optional(),
  description: z.string().optional(),
  ai: AiTier,
  params: z.record(z.string(), ParamSpec),
  /** Read-only and frequent (like tailing a log): not written to the activity log. Safe actions only. */
  quiet: z.boolean().optional(),
});

export const ModuleState = z.enum(['starting', 'running', 'failed', 'stopped']);

export const ModuleInfo = z.object({
  id: z.string().regex(/^[a-z][a-z0-9-]{1,31}$/),
  name: z.string(),
  icon: z.string(),
  version: z.string(),
  state: ModuleState,
  actions: z.array(ActionSpec),
  status: z.record(z.string(), z.unknown()).nullable(),
});

export const ActorKind = z.enum(['user', 'assistant', 'automation', 'phone']);

export const Actor = z.object({
  kind: ActorKind,
  ref: z.string().optional(),
});

export const ActivityEntry = z.object({
  id: z.string(),
  ts: z.iso.datetime(),
  actor: Actor,
  node_id: z.string(),
  module: z.string(),
  action: z.string(),
  params: z.record(z.string(), z.unknown()),
  result: z.enum(['ok', 'error', 'denied']),
  error_code: ErrorCode.optional(),
  duration_ms: z.number().int().nonnegative(),
});

const msg = <T extends string, B extends z.ZodType>(type: T, body: B) =>
  z.object({
    v: z.literal(PROTOCOL_VERSION),
    id: z.string().min(1),
    re: z.string().optional(),
    ts: z.iso.datetime(),
    type: z.literal(type),
    body,
  });

export const Hello = msg(
  'hello',
  z.object({
    client: z.object({ name: z.string(), version: z.string() }),
    token: z.string().min(1),
  }),
);

export const Welcome = msg(
  'welcome',
  z.object({
    node: z.object({ id: z.string(), name: z.string(), version: z.string() }),
    capabilities: z.array(z.string()),
  }),
);

export const CatalogGet = msg('catalog.get', z.object({}));

export const Catalog = msg('catalog', z.object({ modules: z.array(ModuleInfo) }));

export const ActionInvoke = msg(
  'action.invoke',
  z.object({
    module: z.string(),
    action: z.string(),
    params: z.record(z.string(), z.unknown()),
    actor: Actor,
    approval_id: z.string().optional(),
  }),
);

export const ActionResult = msg(
  'action.result',
  z.object({
    ok: z.boolean(),
    result: z.unknown().optional(),
    error: ErrorInfo.optional(),
  }),
);

export const ActivityQuery = msg(
  'activity.query',
  z.object({
    limit: z.number().int().min(1).max(500).optional(),
    module: z.string().optional(),
  }),
);

export const Activity = msg('activity', z.object({ entries: z.array(ActivityEntry) }));

export const ErrorMessage = msg('error', ErrorInfo);

export const Message = z.discriminatedUnion('type', [
  Hello,
  Welcome,
  CatalogGet,
  Catalog,
  ActionInvoke,
  ActionResult,
  ActivityQuery,
  Activity,
  ErrorMessage,
]);
export type Message = z.infer<typeof Message>;
export type MessageType = Message['type'];
