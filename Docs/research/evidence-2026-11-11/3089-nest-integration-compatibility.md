# #3089: Nest comparison package pins and compatibility receipts

G12Docs-2, 2026-11-11. This is the evidence for #3173 c25 (formerly #3089). It
answers [nestjs-integration-version-coherence](../mine-for-jet-2026-09-12.md#finding-nestjs-integration-version-coherence).

## Question

Each package in the Nest comparison set needs an exact pin, plus a receipt from
a real `npm ci` and a start/request run. Where a package's declared peer range
contradicts the pinned core, is that contradiction resolved, or is the package
marked unsupported?

## Method

The pins are the ones the comparison captured on 2026-09-11. These are core,
common, platform-express and swagger 12.0.1, CLI 12.0.0, config 12.0.0, BullMQ
integration 12.0.0, schedule 12.0.1, cache-manager integration 12.0.0, TypeORM
integration 12.0.1, JWT 12.0.1, GraphQL and Apollo 14.0.0, Terminus 12.0.0,
Observe 0.1.8 and Throttler 6.5.0. The comparison also names Prisma as the
other persistence path, so the set adds prisma, @prisma/client and
@prisma/adapter-libsql at 7.10.1, the current release.

Each package's declared peers were read from the registry (`npm view <pkg>@<pin>
peerDependencies engines`). The tools were Node v24.21.0 and npm 11.19.0.

1. Throttler 6.5.0 was installed with every other pin. The command was
   `npm install --package-lock-only`, with no `--force` and no
   `--legacy-peer-deps`.
2. The set was resolved again with Throttler 6.6.0, then installed clean with
   `npm ci --no-audit --no-fund`.
3. `npx prisma generate` and `npx prisma migrate deploy` built the client and
   applied the one committed migration. Then the CLI built the app with
   `npx nest build`. The app is a single module that exercises every package
   (`src/main.ts` below).
4. `run.mjs` (below) ran the app for real. It starts a private Redis 8.8.3 for
   BullMQ and a local HTTP collector as the Observe endpoint. It drives every
   integration over real HTTP, then sends SIGTERM.
   - The secrets are random per run: the JWT secret is 32 random bytes and the
     Observe secret 16. None is a sample value.
   - Each schema comes from explicit migrations: TypeORM runs with
     `synchronize: false`, and Prisma uses `migrate deploy`, never `db push`.

## Pins and verdicts

| Package | Pin | Declared peers that matter | `npm ci` | Start/request receipt | Verdict |
|---|---|---|---|---|---|
| @nestjs/core | 12.0.1 | common ^12, platform-express ^12, rxjs ^7.1 | installed | app ready, every route answered | supported |
| @nestjs/common | 12.0.1 | rxjs ^7.1, reflect-metadata ^0.1.12 or ^0.2 | installed | as core | supported |
| @nestjs/platform-express | 12.0.1 | core and common ^12 | installed | HTTP listener (Express 5.2.1) | supported |
| @nestjs/swagger | 12.0.1 | core and common ^12; Node ^20.19 or >=22.12 | installed | `/docs-json` lists `/orders` | supported |
| @nestjs/cli | 12.0.0 | build tools optional; Node >=20.11 | installed | `nest build` exit 0 | supported |
| @nestjs/config | 12.0.0 | common ^11 or ^12 | installed | `getOrThrow` supplied the JWT secret, Redis port and Observe settings | supported |
| @nestjs/jwt | 12.0.1 | common ^8 to ^12 | installed | no token: 401; signed token: 201 | supported |
| @nestjs/typeorm | 12.0.1 | core and common ^10 to ^12; typeorm ^0.3 | installed (typeorm 0.3.26, sql.js 1.13.0) | migration ran; insert 201, `id=1` | supported |
| @nestjs/terminus | 12.0.0 | core and common ^11 or ^12; integrations optional | installed | `/health` 200, database `up` | supported |
| @nestjs/throttler | 6.5.0 | core and common ^7 to **^11** | **ERESOLVE**: `peer @nestjs/common@"^7.0.0 \|\| … \|\| ^11.0.0" from @nestjs/throttler@6.5.0` against common 12.0.1 | not run | **unsupported** with core 12 |
| @nestjs/throttler | 6.6.0 | core and common ^7 to ^12 | installed | `/limited` with limit 2: 200, 200, 429 | supported; it replaces 6.5.0 |
| @nestjs/cache-manager | 12.0.0 | cache-manager >=6, keyv >=5, rxjs ^7.8.1 | installed (cache-manager 7.2.0, keyv 5.5.0) | value set on order and read back (`1`) | supported |
| @nestjs/schedule | 12.0.1 | core and common ^11 or ^12 | installed | `@Interval` ticks counted | supported |
| @nestjs/bullmq | 12.0.0 | bullmq ^3 to ^6; core and common ^10 to ^12 | installed (bullmq 5.58.0) | job queued, worker marked the order `processed` | supported |
| @nestjs/graphql | 14.0.0 | core and common ^12; graphql ^16.11 or ^17 | installed (graphql 16.11.0) | `{ orderCount }` 200, `orderCount=1` | supported, with the two caveats below |
| @nestjs/apollo | 14.0.0 | @apollo/server ^5; graphql ^16.10 or ^17 | installed (@apollo/server 5.5.0) | as graphql | supported; it needs an undeclared package, below |
| @nestjs/observe | 0.1.8 | core and common ^11 or ^12; bullmq, graphql, schedule and microservices peers | installed (microservices 12.0.1) | 1 batch reached the local collector | supported against a local endpoint, with the shutdown caveat below |
| prisma (CLI) | 7.10.1 | Node ^20.19, ^22.12 or >=24 | installed | `prisma generate` and `prisma migrate deploy` exit 0 (one migration applied) | supported; NixOS needs the engine override below |
| @prisma/client | 7.10.1 | prisma, typescript >=5.4 | installed | `/notes` create 201 `id=1`, count 1, through Nest DI | supported |
| @prisma/adapter-libsql | 7.10.1 | @libsql/client ^0.17 (a dependency) | installed | the client's SQLite driver for the run above | supported |

The receipts, abridged from the run's `receipt.json`:

```json
{
 "ready": true,
 "terminus": "200 up",
 "jwt_denied": 401,
 "typeorm_insert": "201 id=1",
 "bullmq": "processed",
 "cache_manager": 1,
 "schedule_ticks": true,
 "graphql_apollo": "200 orderCount=1",
 "swagger": true,
 "swagger_post_orders_request_schema": false,
 "prisma": "201 id=1 count=1",
 "swagger_post_notes_request_schema": false,
 "throttler": "200,200,429",
 "observe_batches": 1,
 "stderr_before_shutdown": 0,
 "shutdown": "exit=null signal=SIGTERM drained on SIGTERM",
 "stderr_lines": 1,
 "node": "v24.21.0"
}
```

`npm ci` added 535 packages in 8 s.
- The supported set's lockfile (lockfileVersion 3) has sha256
  `be9ff48beba542a3d0486a785a996d7c851f216fcac92c96c568e84d63453a49`.
- Its `package.json` has sha256
  `f983f66d418a0944687e53547f4f6ccc867a206b85f10037f13c21d74601ee08`.

## Contradictions and how each was settled

- **Throttler 6.5.0 with core 12.0.1** is a declared peer contradiction, and npm
  refuses it (ERESOLVE). It is marked unsupported and is not bypassed with
  `--legacy-peer-deps` or `--force`. Throttler 6.6.0 (2026-09-16) widens its
  peers to ^12. It is the supported pin, and the run proves it.
- **Apollo 14 on Express needs `@as-integrations/express5`.** The driver does
  not declare this as a peer. Without it the app does not start:
  `ERROR [PackageLoader] The "@as-integrations/express5" package is missing.
  Please, make sure to install it to use GraphQLModule.` The supported set pins
  it at 1.1.2, whose peers are express ^5 and @apollo/server ^4 or ^5.
- **A global `ThrottlerGuard` breaks GraphQL.** As the global guard it reads the
  request from the HTTP context only. A GraphQL query then failed:
  `TypeError: Cannot read properties of undefined (reading 'ip')`, and
  `orderCount` came back undefined. Two settings are needed together:
  - the GraphQL-aware guard subclass in `src/main.ts`;
  - the Apollo `context: ({ req, res }) => ({ req, res })` option, without which
    the error is `reading 'header'`.
- **Observe's shutdown.** On SIGTERM, the agent's worker thread exits with code
  1 and the agent logs `Worker stopped with exit code 1. Restarting worker...`
  while the app drains. This is the run's only stderr line, and none appears
  before shutdown. The app still drains: `onApplicationShutdown` ran, and the
  process ended by SIGTERM, which Nest's shutdown hooks re-raise.
  - Observe's hosted collector needs an account, which this report does not
    authorize. Only a local endpoint was exercised.
- **Observe's flush interval.** It enforces a 1000 ms minimum: a 200 ms setting
  was raised with a warning.

- **Prisma's schema engine on NixOS.** `prisma generate` and `migrate deploy`
  stop with `Precompiled engine files are not available for nixos` and a 404 for
  `linux-nixos/schema-engine`. Prisma publishes no NixOS engine. The run used
  the engine Prisma publishes for `debian-openssl-3.0.x` at the CLI's own commit
  (`schema-engine-cli d1b3f377ca835d73fd50954a5e22128b9ebdd6b8`), given through
  `PRISMA_SCHEMA_ENGINE_BINARY`. The download's sha256
  `5827f56149193f0249b2f4b8a64e60c300c3129ed6514bad0a11c340e1cffad6` matches
  Prisma's published checksum. It runs through the host's nix-ld loader. Other
  Linux distributions download the engine themselves.

## Documentation generation: Nest compared with Prisma

- In Nest, `SwaggerModule.createDocument` lists every route (`/orders` is
  present). It derives request schemas only from classes it can reflect, through
  `@ApiProperty` or the CLI Swagger plugin. A handler that takes an inline
  object type gets no request schema: `swagger_post_orders_request_schema` is
  `false` above.
- Prisma's generated client types are TypeScript types, not classes, so Swagger
  cannot reflect them either. The run shows it: `/notes` is listed, but its
  POST has no request schema (`swagger_post_notes_request_schema` is `false`).
  A Prisma-backed Nest API needs hand-written DTO classes or a third-party DTO
  generator before its OpenAPI document carries schemas. TypeORM entities are
  classes, but they still need `@ApiProperty` before Swagger describes them.
- In Jet, the OpenAPI document comes from the same `#Codable` types the handler
  decodes (`web.openapi(router)` in the backend battery), with no second
  declaration.

## Inputs

`package.json` (the supported set):

```json
{
  "name": "jet-nest-compat",
  "private": true,
  "version": "0.0.0",
  "scripts": {
    "build": "nest build",
    "start": "node dist/main.js"
  },
  "dependencies": {
    "@apollo/server": "5.5.0",
    "@nestjs/apollo": "14.0.0",
    "@nestjs/bullmq": "12.0.0",
    "@nestjs/cache-manager": "12.0.0",
    "@nestjs/common": "12.0.1",
    "@nestjs/config": "12.0.0",
    "@nestjs/core": "12.0.1",
    "@nestjs/graphql": "14.0.0",
    "@nestjs/jwt": "12.0.1",
    "@nestjs/microservices": "12.0.1",
    "@nestjs/observe": "0.1.8",
    "@nestjs/platform-express": "12.0.1",
    "@nestjs/schedule": "12.0.1",
    "@nestjs/swagger": "12.0.1",
    "@nestjs/terminus": "12.0.0",
    "@nestjs/throttler": "6.6.0",
    "@nestjs/typeorm": "12.0.1",
    "bullmq": "5.58.0",
    "cache-manager": "7.2.0",
    "class-transformer": "0.5.1",
    "class-validator": "0.14.2",
    "graphql": "16.11.0",
    "keyv": "5.5.0",
    "reflect-metadata": "0.2.2",
    "rxjs": "7.8.2",
    "sql.js": "1.13.0",
    "typeorm": "0.3.26",
    "@as-integrations/express5": "1.1.2",
    "@prisma/adapter-libsql": "7.10.1",
    "@prisma/client": "7.10.1"
  },
  "devDependencies": {
    "@nestjs/cli": "12.0.0",
    "@types/node": "24.3.0",
    "prisma": "7.10.1",
    "typescript": "5.9.2"
  }
}
```

The unsupported attempt differs only in `"@nestjs/throttler": "6.5.0"`, without
the Prisma and Express 5 lines, which it never reached.

`prisma/schema.prisma`, `prisma.config.ts` and the one migration:

```prisma
generator client {
  provider     = "prisma-client"
  output       = "../src/generated/prisma"
  moduleFormat = "cjs"
}

datasource db {
  provider = "sqlite"
}

model Note {
  id   Int    @id @default(autoincrement())
  text String
}
```

```ts
import { defineConfig } from "prisma/config";

export default defineConfig({
  schema: "prisma/schema.prisma",
  migrations: { path: "prisma/migrations" },
  datasource: { url: process.env.DATABASE_URL },
});
```

```sql
-- prisma/migrations/20261111000000_init/migration.sql
CREATE TABLE "Note" (
    "id" INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    "text" TEXT NOT NULL
);
```

`prisma/migrations/migration_lock.toml` holds `provider = "sqlite"`.

`tsconfig.json` sets `module` to commonjs, `target` to ES2023,
`experimentalDecorators` and `emitDecoratorMetadata` to true, `outDir` to
`./dist`, `rootDir` to `./src`, and `skipLibCheck` to true. `nest-cli.json`
sets `sourceRoot` to `src`.

`src/main.ts`:

```ts
import "reflect-metadata";
import { Body, Controller, Get, Inject, Injectable, Module, OnApplicationShutdown, Param, Post, UnauthorizedException, Headers } from "@nestjs/common";
import { APP_GUARD, NestFactory } from "@nestjs/core";
import { ConfigModule, ConfigService } from "@nestjs/config";
import { JwtModule, JwtService } from "@nestjs/jwt";
import { InjectRepository, TypeOrmModule } from "@nestjs/typeorm";
import { Column, Entity, MigrationInterface, PrimaryGeneratedColumn, QueryRunner, Repository } from "typeorm";
import { HealthCheck, HealthCheckService, TerminusModule, TypeOrmHealthIndicator } from "@nestjs/terminus";
import { Throttle, ThrottlerGuard, ThrottlerModule } from "@nestjs/throttler";
import { CACHE_MANAGER, CacheModule } from "@nestjs/cache-manager";
import type { Cache } from "cache-manager";
import { Interval, ScheduleModule } from "@nestjs/schedule";
import { BullModule, InjectQueue, Processor, WorkerHost } from "@nestjs/bullmq";
import { Job, Queue } from "bullmq";
import { GqlExecutionContext, GraphQLModule, Query, Resolver } from "@nestjs/graphql";
import { ExecutionContext } from "@nestjs/common";
import { ApolloDriver, ApolloDriverConfig } from "@nestjs/apollo";
import { DocumentBuilder, SwaggerModule } from "@nestjs/swagger";
import { createObserveModule } from "@nestjs/observe";
import { PrismaClient } from "./generated/prisma/client";
import { PrismaLibSql } from "@prisma/adapter-libsql";

const { ObserveModule, ObserveInstrument } = createObserveModule();

@Entity()
class Order {
  @PrimaryGeneratedColumn() id: number;
  @Column() item: string;
  @Column() quantity: number;
  @Column({ default: "queued" }) status: string;
}

// An explicit, reviewed migration: the schema is never synchronized from the entities.
class CreateOrders1762800000000 implements MigrationInterface {
  name = "CreateOrders1762800000000";
  async up(runner: QueryRunner) {
    await runner.query(`CREATE TABLE "order" ("id" integer PRIMARY KEY AUTOINCREMENT NOT NULL, "item" varchar NOT NULL, "quantity" integer NOT NULL, "status" varchar NOT NULL DEFAULT ('queued'))`);
  }
  async down(runner: QueryRunner) {
    await runner.query(`DROP TABLE "order"`);
  }
}

@Injectable()
class Ticks {
  count = 0;
  @Interval(50) tick() { this.count += 1; }
}

@Processor("orders")
class OrderWorker extends WorkerHost {
  constructor(@InjectRepository(Order) private readonly orders: Repository<Order>) { super(); }
  async process(job: Job<{ id: number }>) {
    await this.orders.update(job.data.id, { status: "processed" });
    return job.data.id;
  }
}

@Resolver()
class OrderResolver {
  constructor(@InjectRepository(Order) private readonly orders: Repository<Order>) {}
  @Query("orderCount") async orderCount() { return this.orders.count(); }
}

// Prisma beside TypeORM: the schema comes from `prisma migrate deploy`, never `db push`.
@Injectable()
class PrismaService extends PrismaClient implements OnApplicationShutdown {
  constructor() { super({ adapter: new PrismaLibSql({ url: process.env.DATABASE_URL }) }); }
  async onApplicationShutdown() { await this.$disconnect(); }
}

@Controller()
class AppController implements OnApplicationShutdown {
  constructor(
    private readonly health: HealthCheckService,
    private readonly db: TypeOrmHealthIndicator,
    private readonly jwt: JwtService,
    private readonly ticks: Ticks,
    private readonly prisma: PrismaService,
    @InjectRepository(Order) private readonly orders: Repository<Order>,
    @InjectQueue("orders") private readonly queue: Queue,
    @Inject(CACHE_MANAGER) private readonly cache: Cache,
  ) {}

  @Get("health") @HealthCheck()
  check() { return this.health.check([() => this.db.pingCheck("database")]); }

  @Post("login")
  async login(@Body() body: { user: string }) { return { token: await this.jwt.signAsync({ sub: body.user }) }; }

  @Post("orders")
  async create(@Headers("authorization") auth: string, @Body() body: { item: string; quantity: number }) {
    try { await this.jwt.verifyAsync((auth ?? "").replace(/^Bearer /, "")); } catch { throw new UnauthorizedException(); }
    const saved = await this.orders.save(this.orders.create({ item: body.item, quantity: body.quantity }));
    await this.queue.add("order", { id: saved.id });
    await this.cache.set("last-order", saved.id);
    return { id: saved.id };
  }

  @Get("orders/:id")
  async status(@Param("id") id: string) { return this.orders.findOneByOrFail({ id: Number(id) }); }

  @Get("cache") async cached() { return { last: await this.cache.get("last-order") }; }

  @Get("ticks") ticksSeen() { return { ticks: this.ticks.count }; }

  @Post("notes") async note(@Body() body: { text: string }) { return this.prisma.note.create({ data: { text: body.text } }); }

  @Get("notes") async notes() { return { count: await this.prisma.note.count() }; }

  @Get("limited") @Throttle({ default: { limit: 2, ttl: 60000 } })
  limited() { return { ok: true }; }

  onApplicationShutdown(signal?: string) { console.log(`drained on ${signal}`); }
}

// The default guard reads req/res from HTTP context only; GraphQL requests carry them in the GraphQL context.
@Injectable()
class GqlThrottlerGuard extends ThrottlerGuard {
  getRequestResponse(context: ExecutionContext) {
    if (context.getType<string>() !== "graphql") return super.getRequestResponse(context);
    const ctx = GqlExecutionContext.create(context).getContext();
    return { req: ctx.req, res: ctx.res };
  }
}

@Module({
  imports: [
    ConfigModule.forRoot({ isGlobal: true, ignoreEnvFile: true }),
    JwtModule.registerAsync({ inject: [ConfigService], useFactory: (config: ConfigService) => ({ secret: config.getOrThrow<string>("JWT_SECRET"), signOptions: { expiresIn: "5m" } }) }),
    TypeOrmModule.forRoot({ type: "sqljs", entities: [Order], migrations: [CreateOrders1762800000000], migrationsRun: true, synchronize: false }),
    TypeOrmModule.forFeature([Order]),
    TerminusModule,
    ThrottlerModule.forRoot([{ ttl: 60000, limit: 100 }]),
    CacheModule.register(),
    ScheduleModule.forRoot(),
    BullModule.forRootAsync({ inject: [ConfigService], useFactory: (config: ConfigService) => ({ connection: { host: "127.0.0.1", port: Number(config.getOrThrow<string>("REDIS_PORT")) } }) }),
    BullModule.registerQueue({ name: "orders" }),
    GraphQLModule.forRoot<ApolloDriverConfig>({ driver: ApolloDriver, typeDefs: "type Query { orderCount: Int! }", context: ({ req, res }) => ({ req, res }) }),
    ObserveModule.forRootAsync({ inject: [ConfigService], useFactory: (config: ConfigService) => ({ appKey: config.getOrThrow<string>("OBSERVE_APP_KEY"), appSecret: config.getOrThrow<string>("OBSERVE_APP_SECRET"), serviceId: "jet-nest-compat", endpoint: config.getOrThrow<string>("OBSERVE_ENDPOINT"), flushInterval: 1000 }) }),
  ],
  controllers: [AppController],
  providers: [Ticks, PrismaService, OrderWorker, OrderResolver, { provide: APP_GUARD, useClass: GqlThrottlerGuard }],
})
class AppModule {}

async function bootstrap() {
  const app = await NestFactory.create(AppModule, { instrument: ObserveInstrument, logger: ["error", "warn"] });
  app.enableShutdownHooks();
  SwaggerModule.setup("docs", app, () => SwaggerModule.createDocument(app, new DocumentBuilder().setTitle("orders").build()));
  await app.listen(Number(process.env.PORT ?? 0), "127.0.0.1");
  console.log(`ready ${await app.getUrl()}`);
}
bootstrap();
```

`run.mjs` runs with `DATABASE_URL=file:<dir>/run.db`, after `npm ci`,
`npx prisma generate`, `npx prisma migrate deploy` and `npx nest build`:
`REDIS_SERVER=<redis-server path> node run.mjs`.

```js
// Start/request run for the pinned Nest set: Redis for BullMQ, a local
// collector for @nestjs/observe, then every integration through real HTTP.
import { spawn } from "node:child_process";
import { createServer } from "node:http";
import { randomBytes } from "node:crypto";
import { once } from "node:events";

const receipt = {};
const wait = (ms) => new Promise((done) => setTimeout(done, ms));
const freePort = async () => {
  const server = createServer();
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  const { port } = server.address();
  server.close();
  return port;
};

const redisPort = await freePort();
const redis = spawn(process.env.REDIS_SERVER, ["--port", String(redisPort), "--bind", "127.0.0.1", "--save", "", "--appendonly", "no"], { stdio: "ignore" });
await wait(300);

let collected = 0;
const collector = createServer((req, res) => {
  collected += 1;
  req.resume();
  req.on("end", () => { res.writeHead(200, { "content-type": "application/json" }); res.end("{}"); });
});
collector.listen(0, "127.0.0.1");
await once(collector, "listening");

const app = spawn(process.execPath, ["dist/main.js"], {
  env: {
    ...process.env,
    JWT_SECRET: randomBytes(32).toString("hex"),
    REDIS_PORT: String(redisPort),
    OBSERVE_APP_KEY: "local-run",
    OBSERVE_APP_SECRET: randomBytes(16).toString("hex"),
    OBSERVE_ENDPOINT: `http://127.0.0.1:${collector.address().port}`,
    PORT: "0",
    DATABASE_URL: process.env.DATABASE_URL,
  },
  stdio: ["ignore", "pipe", "pipe"],
});
let out = "";
let err = "";
app.stdout.on("data", (chunk) => { out += chunk; });
app.stderr.on("data", (chunk) => { err += chunk; });
const exited = once(app, "exit");

let base = "";
for (let tries = 0; tries < 200 && !base; tries += 1) {
  const ready = out.match(/ready (http:\/\/\S+)/);
  if (ready) base = ready[1].replace("[::1]", "127.0.0.1");
  else await wait(50);
}
if (!base) {
  console.log(JSON.stringify({ ready: false, out, err }));
  process.exit(1);
}
receipt.ready = true;

const call = async (method, path, body, headers = {}) => {
  const response = await fetch(base + path, { method, headers: { "content-type": "application/json", ...headers }, body: body ? JSON.stringify(body) : undefined });
  return { status: response.status, body: await response.json().catch(() => null) };
};

const health = await call("GET", "/health");
receipt.terminus = `${health.status} ${health.body?.info?.database?.status}`;
receipt.jwt_denied = (await call("POST", "/orders", { item: "book", quantity: 1 })).status;
const token = (await call("POST", "/login", { user: "ada" })).body.token;
const created = await call("POST", "/orders", { item: "book", quantity: 2 }, { authorization: `Bearer ${token}` });
receipt.typeorm_insert = `${created.status} id=${created.body?.id}`;
let status = "";
for (let tries = 0; tries < 100 && status !== "processed"; tries += 1) {
  status = (await call("GET", `/orders/${created.body.id}`)).body?.status;
  if (status !== "processed") await wait(50);
}
receipt.bullmq = status;
receipt.cache_manager = (await call("GET", "/cache")).body?.last;
let ticks = 0;
for (let tries = 0; tries < 40 && ticks === 0; tries += 1) { ticks = (await call("GET", "/ticks")).body?.ticks ?? 0; if (ticks === 0) await wait(50); }
receipt.schedule_ticks = ticks > 0;
const graphql = await call("POST", "/graphql", { query: "{ orderCount }" });
receipt.graphql_apollo = `${graphql.status} orderCount=${graphql.body?.data?.orderCount}`;
const docs = await fetch(base + "/docs-json").then((response) => response.json());
receipt.swagger = Object.keys(docs.paths ?? {}).includes("/orders");
receipt.swagger_post_orders_request_schema = Boolean(docs.paths?.["/orders"]?.post?.requestBody);
const note = await call("POST", "/notes", { text: "first" });
receipt.prisma = `${note.status} id=${note.body?.id} count=${(await call("GET", "/notes")).body?.count}`;
receipt.swagger_post_notes_request_schema = Boolean(docs.paths?.["/notes"]?.post?.requestBody);
const limited = [];
for (let index = 0; index < 3; index += 1) limited.push((await call("GET", "/limited")).status);
receipt.throttler = limited.join(",");
for (let tries = 0; tries < 100 && collected === 0; tries += 1) await wait(50);
receipt.observe_batches = collected;

receipt.stderr_before_shutdown = err.split("\n").filter((line) => line.trim()).length;
app.kill("SIGTERM");
const [code, signal] = await exited;
receipt.shutdown = `exit=${code} signal=${signal} ${out.includes("drained on SIGTERM") ? "drained on SIGTERM" : "no drain line"}`;
receipt.stderr_lines = err.split("\n").filter((line) => line.trim()).length;
redis.kill("SIGTERM");
collector.close();
receipt.node = process.version;
(await import("node:fs")).writeFileSync("receipt.json", JSON.stringify(receipt, null, 1) + "\n");
console.log(JSON.stringify(receipt, null, 1));
if (err.trim()) console.log(err);
```

## Limits

- This is one run on one Linux host (Node v24.21.0, Redis 8.8.3).
- Observe was exercised against a local collector only, never the hosted
  service.
- Prisma ran with SQLite through the libSQL adapter only, not with a server
  database.
- A newer patch of any package may change these verdicts. These pins are the
  ones the comparison names. The only changes are the Throttler 6.6.0
  resolution, the Express 5 integration, and the added Prisma set.
