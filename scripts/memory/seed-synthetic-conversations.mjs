import { randomUUID } from "node:crypto";
import { mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";

const MANIFEST_NAME = "mnemora-synthetic-manifest.json";
const PROFILES = {
  normal: { count: 1, messages: 60, contentBytes: 2048, workflow: false },
  heavy: { count: 1, messages: 240, contentBytes: 8192, workflow: true },
  max: { count: 1, messages: 480, contentBytes: 16384, workflow: true },
  sidebar: { count: 120, messages: 4, contentBytes: 512, workflow: false },
};

const args = parseArgs(process.argv.slice(2));
if (args.help || (!args.profile && !args.cleanup)) {
  printUsage();
  process.exit(args.help ? 0 : 1);
}

const appDataDir = args.appDataDir ? resolve(args.appDataDir) : null;
if (!appDataDir) {
  throw new Error("--app-data-dir is required so synthetic data cannot target the wrong profile.");
}

const conversationDir = join(appDataDir, "conversations");
const manifestPath = args.manifest
  ? resolve(args.manifest)
  : join(conversationDir, MANIFEST_NAME);
await mkdir(conversationDir, { recursive: true });

if (args.cleanup) {
  await cleanup(conversationDir, manifestPath);
  process.stdout.write("Removed synthetic conversations recorded in " + manifestPath + "\n");
  process.exit(0);
}

const profile = PROFILES[args.profile];
if (!profile) {
  throw new Error(
    "Unknown profile '" + args.profile + "'. Choose: " + Object.keys(PROFILES).join(", ") + ".",
  );
}

const count = positiveInt(args.count, profile.count, "count");
const messagesPerConversation = positiveInt(args.messages, profile.messages, "messages");
const contentBytes = positiveInt(args.contentBytes, profile.contentBytes, "content-bytes");
if (messagesPerConversation > 500) {
  throw new Error("messages cannot exceed the product limit of 500 per conversation.");
}
if (contentBytes > 1024 * 1024) {
  throw new Error("content-bytes cannot exceed the product limit of 1 MiB per message.");
}

const existingManifest = await readManifest(manifestPath);
const createdAt = Date.now();
const files = [];
const conversations = [];

for (let conversationIndex = 0; conversationIndex < count; conversationIndex += 1) {
  const id = "synthetic-" + args.profile + "-" + randomUUID();
  const conversation = createConversation({
    id,
    profile: args.profile,
    conversationIndex,
    messagesPerConversation,
    contentBytes,
    workflow: profile.workflow,
    createdAt: createdAt + conversationIndex,
  });
  const fileName = "conv_" + id + ".json";
  await writeFile(
    join(conversationDir, fileName),
    JSON.stringify(conversation, null, 2) + "\n",
    { encoding: "utf8", flag: "wx" },
  );
  files.push(fileName);
  conversations.push({
    id,
    title: conversation.title,
    messageCount: conversation.messages.length,
  });
}

const manifest = {
  schemaVersion: 1,
  createdAt,
  appDataDir,
  files: existingManifest.files.concat(files),
  conversations: existingManifest.conversations.concat(conversations),
};
await mkdir(dirname(manifestPath), { recursive: true });
await writeFile(manifestPath, JSON.stringify(manifest, null, 2) + "\n", "utf8");

process.stdout.write([
  "Created " + files.length + " synthetic conversation file(s).",
  "Profile: " + args.profile + ".",
  "Messages per conversation: " + messagesPerConversation + ".",
  "Content bytes per message: " + contentBytes + ".",
  "Manifest: " + manifestPath,
  "Restart Mnemora before opening the generated conversations so the index is rebuilt.",
].join("\n") + "\n");

function createConversation(options) {
  const messages = [];
  for (let index = 0; index < options.messagesPerConversation; index += 1) {
    const role = index % 2 === 0 ? "user" : "assistant";
    const timestamp = options.createdAt + index;
    const hasWorkflow = options.workflow && role === "assistant" && index % 6 === 1;
    messages.push({
      id: options.id + "-message-" + (index + 1),
      conversationId: options.id,
      role,
      content: buildMarkdown(
        options.profile,
        options.conversationIndex,
        index,
        role,
        options.contentBytes,
      ),
      attachments: [],
      literatureReferences: [],
      noteReferences: [],
      reasoning: role === "assistant"
        ? repeatAscii(
          "Reasoning checkpoint. ",
          Math.min(2048, Math.floor(options.contentBytes / 4)),
        )
        : null,
      status: "completed",
      createdAt: timestamp,
      updatedAt: timestamp,
      modelId: null,
      modelSnapshot: null,
      usage: null,
      activatedSkills: [],
      toolTraces: hasWorkflow
        ? createToolTraces(options.id, index, options.contentBytes)
        : [],
      agentRunId: hasWorkflow ? options.id + "-run-" + index : null,
      workflowSummary: hasWorkflow
        ? {
          status: "completed",
          stepCount: 5,
          toolCallCount: 3,
          skillCount: 1,
          durationMs: 780,
        }
        : null,
      errorMessage: null,
    });
  }

  return {
    id: options.id,
    title: "Synthetic " + options.profile + " conversation " + (options.conversationIndex + 1),
    messages,
    assistantId: null,
    providerId: null,
    modelId: null,
    systemPrompt: "",
    contextSummary: "",
    compressedUntilMessageId: null,
    contextCompressionCount: 0,
    enabledSkillIds: [],
    linkedLibraryItemIds: [],
    permissionMode: "askSensitive",
    projectId: null,
    collectionId: null,
    pinned: false,
    createdAt: options.createdAt,
    updatedAt: options.createdAt + messages.length,
  };
}

function buildMarkdown(profile, conversationIndex, messageIndex, role, byteLength) {
  const base = [
    "# Synthetic " + profile + " message",
    "",
    "Generated pressure-test data: conversation " + (conversationIndex + 1)
      + ", message " + (messageIndex + 1) + ", role " + role + ".",
    "",
    "## Research notes",
    "- Stable markdown headings and lists",
    "- A fenced code block for syntax highlighting",
    "- A Mermaid block for deferred diagram rendering",
    "- A table and inline emphasis for layout coverage",
    "",
    "~~~typescript",
    "export function syntheticSample(value: number) {",
    "  return { value, stable: true, source: 'memory-benchmark' };",
    "}",
    "~~~",
    "",
    "~~~mermaid",
    "graph TD",
    "  A[Input] --> B[Parse]",
    "  B --> C[Render]",
    "  C --> D[Observe]",
    "~~~",
    "",
    "| metric | value |",
    "| --- | ---: |",
    "| message | " + (messageIndex + 1) + " |",
    "| source | synthetic |",
    "",
    "Repeated payload: ",
  ].join("\n");
  return repeatAscii(base, byteLength);
}

function createToolTraces(conversationId, messageIndex, contentBytes) {
  return [0, 1, 2].map((traceIndex) => ({
    callId: conversationId + "-call-" + messageIndex + "-" + traceIndex,
    name: traceIndex === 0
      ? "conversation_search"
      : traceIndex === 1
        ? "memory_read"
        : "document_outline",
    status: "completed",
    risk: traceIndex === 1 ? "memoryRead" : "conversationRead",
    argumentSummary: "Synthetic tool arguments " + messageIndex + "-" + traceIndex,
    preview: repeatAscii(
      "Synthetic tool output. ",
      Math.min(500, Math.floor(contentBytes / 16)),
    ),
    durationMs: 120 + traceIndex * 30,
    inputChars: 120,
    outputChars: Math.min(contentBytes, 4000),
    outputTruncated: contentBytes > 4000,
    errorKind: null,
  }));
}

function repeatAscii(seed, byteLength) {
  const target = Math.max(seed.length, byteLength);
  return seed.repeat(Math.ceil(target / seed.length)).slice(0, target);
}

function positiveInt(value, fallback, label) {
  if (value === undefined) return fallback;
  const parsed = Number(value);
  if (!Number.isInteger(parsed) || parsed < 1) {
    throw new Error("--" + label + " must be a positive integer.");
  }
  return parsed;
}

function parseArgs(values) {
  const parsed = {};
  for (let index = 0; index < values.length; index += 1) {
    const value = values[index];
    if (value === "--cleanup" || value === "--help") {
      parsed[value.slice(2)] = true;
      continue;
    }
    if (!value.startsWith("--")) throw new Error("Unexpected argument '" + value + "'.");
    const key = value
      .slice(2)
      .replace(/-([a-z])/g, (_, character) => character.toUpperCase());
    const next = values[index + 1];
    if (!next || next.startsWith("--")) throw new Error("Missing value for " + value + ".");
    parsed[key] = next;
    index += 1;
  }
  return parsed;
}

async function readManifest(path) {
  try {
    const value = JSON.parse(await readFile(path, "utf8"));
    return {
      files: Array.isArray(value.files) ? value.files : [],
      conversations: Array.isArray(value.conversations) ? value.conversations : [],
    };
  } catch {
    return { files: [], conversations: [] };
  }
}

async function cleanup(conversationDir, manifestPath) {
  const manifest = await readManifest(manifestPath);
  for (const fileName of manifest.files) {
    if (
      typeof fileName === "string"
      && /^conv_synthetic-[a-z]+-[a-f0-9-]+\.json$/i.test(fileName)
    ) {
      await rm(join(conversationDir, fileName), { force: true });
    }
  }
  await rm(manifestPath, { force: true });
  await rm(join(conversationDir, "index.json"), { force: true });
}

function printUsage() {
  process.stdout.write([
    "Synthetic conversation generator",
    "",
    "Usage:",
    "  node scripts/memory/seed-synthetic-conversations.mjs --app-data-dir <dir> --profile <normal|heavy|max|sidebar>",
    "  node scripts/memory/seed-synthetic-conversations.mjs --app-data-dir <dir> --cleanup",
    "",
    "Optional overrides: --count <n> --messages <n> --content-bytes <n> --manifest <path>",
    "",
  ].join("\n"));
}
