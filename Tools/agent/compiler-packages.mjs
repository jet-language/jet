// The Jet compiler in package form: the packages `jet build Compiler` compiles
// (Compiler/JetCli and every package its package.jet path dependencies reach),
// their member source files, and the inputs that make up the compiler's source
// identity.
//
// Package membership follows the driver's source walk (JetCli Request.jet
// `jet_cli_walk_held_sources`): every `.jet` file under the package directory,
// recursively, except dot names, the skipped build names, symlinks, `#!` scripts,
// the reserved manifest names, and directories that are projects of their own
// (they hold a package.jet, pkg.jet, workspace.jet or .jet/lock).
import { createHash } from 'node:crypto';
import { existsSync, lstatSync, readdirSync, readFileSync } from 'node:fs';
import { join, relative, resolve, sep } from 'node:path';

const ENTRY_PACKAGE = 'Compiler/JetCli';
// The workspace files `jet build Compiler` reads beside the packages.
export const WORKSPACE_FILES = ['Compiler/package.jet', 'Compiler/build.jet'];
const SKIPPED_NAMES = new Set(['target', 'build', 'node_modules', 'bin']);
const PROJECT_MARKERS = ['package.jet', 'pkg.jet', 'workspace.jet', '.jet/lock'];
const RESERVED_SOURCE_NAMES = new Set(['package.jet', 'pkg.jet', 'env.jet', 'config.jet', 'workspace.jet']);

const repoPath = (repo, path) => relative(repo, path).split(sep).join('/');

function isProject(directory) {
    return PROJECT_MARKERS.some(marker => existsSync(join(directory, marker)));
}

// Every file of the package tree at `directory` (any extension), repo-relative,
// sorted: the walk the driver makes, before its `.jet` filter.
function packageTree(repo, directory) {
    const files = [];
    const walk = path => {
        for (const name of readdirSync(path).sort()) {
            if (name.startsWith('.') || SKIPPED_NAMES.has(name)) continue;
            const child = join(path, name);
            const stat = lstatSync(child);
            if (stat.isSymbolicLink()) continue;
            if (stat.isDirectory()) { if (!isProject(child)) walk(child); }
            else if (stat.isFile()) files.push(repoPath(repo, child));
        }
    };
    walk(directory);
    return files;
}

// The packages `entry` (a repo-relative package directory; the compiler's is
// Compiler/JetCli) depends on, itself included, by package name, each with its
// directory (absolute), in dependency order: every package after the packages
// it depends on.
export function compilerPackages(repo, entry = ENTRY_PACKAGE) {
    const root = resolve(repo);
    const packages = new Map(), visiting = new Set();
    const visit = directory => {
        const manifest = readFileSync(join(directory, 'package.jet'), 'utf8');
        const name = /^name:\s*"([^"]+)"/m.exec(manifest)?.[1];
        if (!name) throw Error(`${repoPath(root, directory)}/package.jet names no package`);
        if (packages.has(name)) return;
        if (visiting.has(name)) throw Error(`package dependency cycle through ${name}`);
        visiting.add(name);
        const deps = /^deps:\s*\{([^}]*)\}/m.exec(manifest)?.[1] ?? '';
        for (const [, path] of deps.matchAll(/^\s*\w+:\s*(\.{1,2}\/[^,\s]*|\.{1,2}),?\s*$/gm)) visit(resolve(directory, path));
        packages.set(name, directory);
    };
    visit(join(root, entry));
    return packages;
}

// The member `.jet` files of each package, repo-relative, in package dependency
// order and sorted within a package: the sources `jet build Compiler` compiles.
export function compilerSources(repo, packages = compilerPackages(repo)) {
    const root = resolve(repo);
    return [...packages].map(([name, directory]) => ({
        name,
        directory: repoPath(root, directory),
        files: packageTree(root, directory).filter(path => {
            const base = path.slice(path.lastIndexOf('/') + 1);
            return base.endsWith('.jet') && !RESERVED_SOURCE_NAMES.has(base) && !readFileSync(join(root, path), 'utf8').startsWith('#!/');
        }),
    }));
}

// The repo-relative roots of the compiler's source identity: the workspace
// files, every compiler package directory, and Core (the toolchain sources the
// compiler reads through JET_TOOLCHAIN_ROOT).
export function compilerInputRoots(repo, packages = compilerPackages(repo)) {
    const root = resolve(repo);
    return [...WORKSPACE_FILES, ...[...packages.values()].map(directory => repoPath(root, directory)), 'Core'];
}

// Every file the compiler's source identity covers, repo-relative, sorted by
// bytes: the workspace files, each package tree (all files, so data a package
// reads at compile time counts), and every file under Core except dot names.
export function compilerInputs(repo, roots = compilerInputRoots(repo)) {
    const root = resolve(repo);
    const files = new Set();
    for (const path of roots) {
        const absolute = join(root, path);
        if (!existsSync(absolute)) continue;
        if (lstatSync(absolute).isFile()) { files.add(path); continue; }
        for (const file of packageTree(root, absolute)) files.add(file);
    }
    return [...files].sort((left, right) => Buffer.compare(Buffer.from(left), Buffer.from(right)));
}

// The compiler's source identity, `name@version#<id>` (the CLI's JET_CLI_BINARY
// and JET_CLI_VERSION): SHA-256 over the domain `jet.compiler-source.v2`, a NUL,
// then for every compiler input in byte order its big-endian u64 path length,
// path, u64 byte length and bytes. A chain stage passes it to the compiler it
// runs as JET_COMPILER_IDENTITY.
export function compilerSourceIdentity(repo) {
    const root = resolve(repo);
    const hash = createHash('sha256').update('jet.compiler-source.v2\0');
    const length = value => { const bytes = Buffer.alloc(8); bytes.writeBigUInt64BE(BigInt(value)); return bytes; };
    for (const path of compilerInputs(root)) {
        const bytes = readFileSync(join(root, path));
        hash.update(length(Buffer.byteLength(path))).update(path).update(length(bytes.length)).update(bytes);
    }
    const registry = readFileSync(join(root, 'Compiler/JetCli/Source/Cli/Registry.jet'), 'utf8');
    const name = /^JET_CLI_BINARY :: "([^"]+)"$/m.exec(registry)?.[1];
    const version = /^JET_CLI_VERSION :: "([^"]+)"$/m.exec(registry)?.[1];
    if (!name || !version) throw Error('Compiler/JetCli/Source/Cli/Registry.jet declares no JET_CLI_BINARY or JET_CLI_VERSION');
    return `${name}@${version}#${hash.digest('hex')}`;
}

// Whether the repo-relative `path` (which may no longer exist) is a compiler
// input under `roots`: the same exclusions as the walk, judged on the path.
export function isCompilerInput(repo, path, roots = compilerInputRoots(repo)) {
    const root = resolve(repo);
    return roots.some(input => {
        if (path === input) return true;
        if (!path.startsWith(`${input}/`)) return false;
        const parts = path.slice(input.length + 1).split('/');
        if (parts.some(part => part.startsWith('.') || SKIPPED_NAMES.has(part))) return false;
        let directory = join(root, input);
        for (const part of parts.slice(0, -1)) {
            directory = join(directory, part);
            if (isProject(directory)) return false;
        }
        return true;
    });
}

// One namespace of the member files of the packages at `entries` (repo-relative
// package directories) and of every package they depend on, for focused
// fixtures that compile production code with a fixture appended: each package
// `use` line is blanked to spaces (offsets and lines stay those of the source),
// and a Core import repeated in a later file is blanked too. Files are in
// package dependency order; a package's own entry (its root `run.jet`, the CLI's
// `fn run`) stays out, so the fixture supplies the unit's entry.
export function compilerFixtureUnit(repo, entries = [ENTRY_PACKAGE]) {
    const packages = new Map(entries.flatMap(entry => [...compilerPackages(repo, entry)]));
    const names = [...compilerPackages(repo).keys(), ...packages.keys()];
    const files = compilerSources(repo, packages).flatMap(entry => entry.files.filter(file => file !== `${entry.directory}/run.jet`));
    return fixtureUnit(repo, files, [...new Set(names)]);
}

// `files` (repo-relative) joined into one namespace, blanking `use` lines of the
// named packages and repeated Core imports.
export function fixtureUnit(repo, files, packageNames = [...compilerPackages(repo).keys()]) {
    const root = resolve(repo);
    const packageImport = new RegExp(`^use (?:${packageNames.join('|')})\\.(?:\\[[^\\]]*\\]|[A-Za-z_][A-Za-z0-9_]*[ \\t]*$)`, 'gm');
    const blank = text => text.replace(/[^\n]/g, ' ');
    const seenImports = new Set(), seenMembers = new Set();
    // A repeated member of a grouped Core import is blanked, keeping every
    // surviving token at its offset; forms this does not recognize stay as written.
    const memberImports = (importText, module, members) => {
        const parts = members.split(',');
        const bindings = parts.map(part => part.match(/^\s*([A-Za-z_][A-Za-z0-9_]*)(?:\s+as\s+([A-Za-z_][A-Za-z0-9_]*))?\s*$/));
        if (bindings.some((binding, index) => !binding && parts[index].trim() !== '')) return importText;
        let removed = false;
        const kept = bindings.map(binding => {
            if (!binding) return false;
            const key = `${module}.${binding[1]} as ${binding[2] ?? binding[1]}`;
            if (seenMembers.has(key)) { removed = true; return false; }
            seenMembers.add(key);
            return true;
        });
        if (!removed) return importText;
        const last = kept.lastIndexOf(true);
        if (last < 0) return blank(importText);
        const rewritten = parts.map((part, index) => (kept[index] ? part : blank(part)) + (index === parts.length - 1 ? '' : kept[index] && index < last ? ',' : ' ')).join('');
        return importText.slice(0, importText.indexOf('[') + 1) + rewritten + ']';
    };
    return files.map(path => {
        const text = readFileSync(join(root, path), 'utf8')
            .replace(packageImport, blank)
            .replace(/^use (core(?:\.[A-Za-z_][A-Za-z0-9_]*)*)\.\[([^\]]*)\]/gm, memberImports)
            .replace(/^use core\.[^\[\n]*$/gm, line => {
                const key = line.trimEnd();
                if (seenImports.has(key)) return blank(line);
                seenImports.add(key);
                return line;
            });
        return text.endsWith('\n') ? text : `${text}\n`;
    }).join('');
}
