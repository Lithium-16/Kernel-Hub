import { writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { z } from 'zod';
import { Message } from './index.ts';

const out = fileURLToPath(new URL('../schema/protocol.schema.json', import.meta.url));
const schema = z.toJSONSchema(Message, { target: 'draft-2020-12' });
writeFileSync(out, `${JSON.stringify(schema, null, 2)}\n`);
console.log(`wrote ${out}`);
