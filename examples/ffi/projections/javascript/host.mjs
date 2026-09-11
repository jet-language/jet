import {
    ResourceDocument,
    RESOURCE_OK,
    RESOURCE_EXPIRED_VIEW,
    RESOURCE_CLOSED,
    add,
    label,
    project_component,
    load,
} from "./target/bindings/projection_javascript.mjs";

const library = load(process.env.JET_NODE_ADDON ?? "./target/libprojection_javascript.node");
const document = new ResourceDocument(library);
if (document.open(Uint8Array.from([10, 20, 30])) !== RESOURCE_OK) process.exitCode = 1;
const staleResult = document.bytes();
if (staleResult.status !== RESOURCE_OK) process.exitCode = 1;
const initialAt = document.at(staleResult.view, 1);
if (initialAt.status !== RESOURCE_OK || initialAt.value !== 20n) process.exitCode = 1;
if (document.replace(Uint8Array.from([40, 50, 60])) !== RESOURCE_OK) process.exitCode = 1;
if (document.at(staleResult.view, 1).status !== RESOURCE_EXPIRED_VIEW) process.exitCode = 1;
const freshResult = document.bytes();
if (freshResult.status !== RESOURCE_OK || document.at(freshResult.view, 1).value !== 50n) process.exitCode = 1;
if (document.close() !== RESOURCE_OK || document.close() !== RESOURCE_CLOSED) process.exitCode = 1;
if (add(library, 41n) !== 42n) process.exitCode = 1;
if (label(library, "hello").toString("utf8") !== "hello") process.exitCode = 1;
const projected = project_component(library, { count: 7, label: "ok" });
if (projected.count !== 7 || projected.label !== "ok") process.exitCode = 1;
