# Web Job Conversation Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a read-only Web Job conversation subpage and expose dispatched subagent/job history from the parent chat thread.

**Architecture:** The server owns all job/thread resolution. Web code asks product-level APIs for a parent thread's dispatched jobs and for a read-only job conversation by `jobId`; it never derives job runtime session IDs or calls scheduler tool actions. The `/chat` page displays persisted dispatched jobs, while `/chat/jobs/:jobId` reuses the existing message presentation without a composer.

**Tech Stack:** Rust workspace with Axum server routes, `argus-wing`/`argus-job`/`argus-session` boundaries, SQLite/Postgres repositories, Vue 3 + Vue Router + Vite + Vitest, OpenTiny Vue, TinyRobot presentation components.

---

### Task 1: Add Server DTOs and Route Skeletons

**Files:**
- Modify: `crates/argus-server/src/routes/mod.rs`
- Modify: `crates/argus-server/src/routes/chat.rs`
- Modify: `crates/argus-server/tests/chat_api.rs`

**Step 1: Write failing route tests**

Add tests near the existing chat API tests:

```rust
#[tokio::test]
async fn chat_job_message_post_route_does_not_exist() {
    let ctx = support::TestContext::new().await;
    let response = ctx
        .post_json("/api/v1/chat/jobs/job-123/messages", &serde_json::json!({
            "message": "should not be accepted"
        }))
        .await;

    assert_eq!(response.status(), axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn list_thread_jobs_requires_valid_ids() {
    let ctx = support::TestContext::new().await;
    let response = ctx
        .get("/api/v1/chat/sessions/not-a-uuid/threads/not-a-thread/jobs")
        .await;

    assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn get_chat_job_requires_non_empty_job_id() {
    let ctx = support::TestContext::new().await;
    let response = ctx.get("/api/v1/chat/jobs/%20").await;

    assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
}
```

If `TestContext` does not expose `post_json` for unknown paths exactly as expected, adapt to the existing helper names in `crates/argus-server/tests/support`.

**Step 2: Run tests to verify failure**

Run:

```bash
cargo test -p argus-server --test chat_api chat_job_message_post_route_does_not_exist list_thread_jobs_requires_valid_ids get_chat_job_requires_non_empty_job_id
```

Expected: at least the new job routes fail with `404` instead of validation behavior or missing handler compile errors.

**Step 3: Add DTOs and route handlers**

In `crates/argus-server/src/routes/chat.rs`, add:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatThreadJobSummaryResponse {
    pub job_id: String,
    pub title: String,
    pub subagent_name: String,
    pub status: String,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub result_preview: Option<String>,
    pub bound_thread_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatJobConversationResponse {
    pub job_id: String,
    pub title: String,
    pub status: String,
    pub thread_id: Option<String>,
    pub session_id: Option<String>,
    pub parent_session_id: Option<String>,
    pub parent_thread_id: Option<String>,
    pub messages: Vec<ChatMessage>,
    pub turn_count: u32,
    pub token_count: u32,
    pub plan_item_count: u32,
}
```

Add handlers:

```rust
pub async fn list_thread_jobs(
    request_user: RequestUser,
    State(state): State<AppState>,
    Path((session_id, thread_id)): Path<(String, String)>,
) -> Result<Json<Vec<ChatThreadJobSummaryResponse>>, ApiError> {
    let jobs = state
        .core()
        .list_chat_thread_jobs(
            &request_user,
            parse_session_id(&session_id)?,
            parse_thread_id(&thread_id)?,
        )
        .await?;
    Ok(Json(jobs.into_iter().map(Into::into).collect()))
}

pub async fn get_chat_job(
    request_user: RequestUser,
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> Result<Json<ChatJobConversationResponse>, ApiError> {
    let conversation = state
        .core()
        .get_chat_job_conversation(&request_user, required_non_empty("job_id", job_id)?)
        .await?;
    Ok(Json(conversation.into()))
}
```

The `.into()` calls will fail until Task 3 adds core DTOs and conversion impls.

In `crates/argus-server/src/routes/mod.rs`, add:

```rust
.route(
    "/api/v1/chat/sessions/{session_id}/threads/{thread_id}/jobs",
    get(chat::list_thread_jobs),
)
.route("/api/v1/chat/jobs/{job_id}", get(chat::get_chat_job))
```

Do not add a `POST /api/v1/chat/jobs/{job_id}/messages` route.

**Step 4: Run tests**

Run:

```bash
cargo test -p argus-server --test chat_api chat_job_message_post_route_does_not_exist list_thread_jobs_requires_valid_ids get_chat_job_requires_non_empty_job_id
```

Expected: compile fails on missing `ServerCore` methods/conversions, proving routes are wired to the intended boundary.

**Step 5: Commit**

```bash
git add crates/argus-server/src/routes/mod.rs crates/argus-server/src/routes/chat.rs crates/argus-server/tests/chat_api.rs
git commit -m "test: sketch web job conversation api routes"
```

### Task 2: Add Job Lookup Support in the Core Boundary

**Files:**
- Modify: `crates/argus-wing/src/lib.rs`
- Modify: `crates/argus-job/src/job_manager/mod.rs`
- Modify: `crates/argus-job/src/job_manager/binding_recovery.rs`
- Modify: `crates/argus-job/src/types.rs`
- Test: `crates/argus-job/src/job_manager/tests/recovery.rs`
- Test: `crates/argus-wing/src/lib.rs`

**Step 1: Write failing job manager and wing tests**

In `crates/argus-job/src/job_manager/tests/recovery.rs`, add a test that creates persisted child metadata and verifies direct children can be recovered by parent thread. Use existing helper patterns in that file and assert returned `RecoveredChildJob` includes `job_id` and `thread_id`.

In `crates/argus-wing/src/lib.rs`, add a test near `dispatch_job_binds_real_thread_id_and_keeps_it_recoverable`:

```rust
#[tokio::test]
async fn dispatched_jobs_for_thread_recovers_child_job_bindings() {
    // Arrange like dispatch_job_binds_real_thread_id_and_keeps_it_recoverable.
    // Dispatch a job from a parent thread.
    // Act: call core.dispatched_jobs_for_thread(parent_thread_id).await.
    // Assert: the list contains the job_id, execution thread_id, and subagent metadata.
}
```

**Step 2: Run tests to verify failure**

Run:

```bash
cargo test -p argus-job recovery::recover_parent_then_children_keeps_persisted_job_id_authoritative
cargo test -p argus-wing dispatched_jobs_for_thread_recovers_child_job_bindings
```

Expected: the new wing method does not exist.

**Step 3: Add public core types and methods**

In `crates/argus-job/src/types.rs`, extend `RecoveredChildJob` if needed:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveredChildJob {
    pub thread_id: ThreadId,
    pub job_id: String,
}
```

If already identical, leave it untouched.

In `crates/argus-job/src/job_manager/mod.rs`, expose a public async method that delegates to existing recovery:

```rust
pub async fn recover_child_jobs_for_thread(
    &self,
    parent_thread_id: ThreadId,
) -> Result<Vec<RecoveredChildJob>, JobError> {
    self.recover_child_jobs_for_thread(parent_thread_id).await
}
```

If the internal method already has that exact name and cannot be overloaded, rename the internal method to `recover_child_jobs_for_thread_inner` and update internal callers in `crates/argus-session/src/manager.rs` and job manager modules.

In `crates/argus-wing/src/lib.rs`, add:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchedJobBinding {
    pub job_id: String,
    pub thread_id: ThreadId,
}

pub async fn dispatched_jobs_for_thread(
    &self,
    parent_thread_id: ThreadId,
) -> Result<Vec<DispatchedJobBinding>> {
    let children = self
        .job_manager
        .recover_child_jobs_for_thread(parent_thread_id)
        .await?;
    Ok(children
        .into_iter()
        .map(|child| DispatchedJobBinding {
            job_id: child.job_id,
            thread_id: child.thread_id,
        })
        .collect())
}
```

Keep this as binding data only. Status/result enrichment belongs in `ServerCore`, where repository records are already available.

**Step 4: Run tests**

Run:

```bash
cargo test -p argus-job recovery
cargo test -p argus-wing dispatched_jobs_for_thread_recovers_child_job_bindings
```

Expected: PASS.

**Step 5: Commit**

```bash
git add crates/argus-job crates/argus-wing crates/argus-session
git commit -m "feat: expose dispatched job bindings"
```

### Task 3: Implement ServerCore Read Models

**Files:**
- Modify: `crates/argus-server/src/server_core.rs`
- Modify: `crates/argus-server/src/routes/chat.rs`
- Test: `crates/argus-server/tests/chat_api.rs`

**Step 1: Write failing API behavior tests**

Add tests in `crates/argus-server/tests/chat_api.rs`:

```rust
#[tokio::test]
async fn thread_jobs_lists_dispatched_subagents_for_parent_thread() {
    // Create a chat session/thread, dispatch a scheduler job through the runtime helper
    // or seed the repository/job metadata using existing support helpers.
    // GET /api/v1/chat/sessions/{session_id}/threads/{thread_id}/jobs
    // Assert one item with job_id, status, subagent_name, and bound_thread_id.
}

#[tokio::test]
async fn get_chat_job_returns_readonly_conversation_messages() {
    // Arrange a bound job thread with messages.
    // GET /api/v1/chat/jobs/{job_id}
    // Assert messages are present and response includes parent_thread_id.
}

#[tokio::test]
async fn get_chat_job_reports_pending_when_job_has_no_thread_binding() {
    // Arrange a JobRecord without thread_id.
    // GET /api/v1/chat/jobs/{job_id}
    // Assert status is pending, thread_id is null, messages is empty.
}
```

Use existing support helpers for creating users/sessions where possible. If direct dispatch is too slow for an API test, seed repository records and trace metadata in the support fixture, but keep SQL writes inside repository helpers.

**Step 2: Run tests to verify failure**

Run:

```bash
cargo test -p argus-server --test chat_api thread_jobs_lists_dispatched_subagents_for_parent_thread get_chat_job_returns_readonly_conversation_messages get_chat_job_reports_pending_when_job_has_no_thread_binding
```

Expected: FAIL or compile fail until core methods are implemented.

**Step 3: Add read model structs**

In `crates/argus-server/src/server_core.rs`, add:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatThreadJobSummary {
    pub job_id: String,
    pub title: String,
    pub subagent_name: String,
    pub status: String,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub result_preview: Option<String>,
    pub bound_thread_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatJobConversation {
    pub job_id: String,
    pub title: String,
    pub status: String,
    pub thread_id: Option<String>,
    pub session_id: Option<String>,
    pub parent_session_id: Option<String>,
    pub parent_thread_id: Option<String>,
    pub messages: Vec<ChatMessage>,
    pub turn_count: u32,
    pub token_count: u32,
    pub plan_item_count: u32,
}
```

Add conversion impls in `routes/chat.rs` from these core structs to route response structs.

**Step 4: Implement read methods**

In `ServerCore`, add:

```rust
pub async fn list_chat_thread_jobs(
    &self,
    request_user: &RequestUser,
    session_id: SessionId,
    parent_thread_id: ThreadId,
) -> Result<Vec<ChatThreadJobSummary>> {
    let user_id = self.resolve_chat_user(request_user).await?;
    self.session_manager
        .get_thread_snapshot_for_user(user_id, session_id, &parent_thread_id)
        .await?;

    let bindings = self.dispatched_jobs_for_parent(parent_thread_id).await?;
    let repository = ArgusSqlite::new(/* existing pool field or repository handle */);
    // Prefer using the existing injected JobRepository. If ServerCore does not retain it,
    // add `job_repo: Arc<dyn JobRepository>` to ServerCore and store it in from_repositories.
}
```

Important implementation notes:

- Add `job_repo: Arc<dyn JobRepository>` and `thread_repo: Arc<dyn ThreadRepository>` fields to `ServerCore` if needed. They are already passed into `from_repositories`; retaining them keeps SQL behind repository traits.
- For each binding, load `JobRecord` with `JobRepository::get`.
- `subagent_name` should prefer `record.result.agent_display_name`, then `record.name`, then `job:<id>`.
- `result_preview` should be a trimmed first 240 characters of `record.result.message`.
- `created_at` can use `record.started_at.or(record.scheduled_at)` if no dedicated created timestamp is exposed by `JobRecord`. Do not add schema churn only for display.
- `updated_at` should prefer `finished_at`, then `started_at`, then `scheduled_at`.

For `get_chat_job_conversation`:

- Resolve chat user first.
- Load `JobRecord` by `job_id`.
- If missing, return `ArgusError::JobNotFound` or an existing not-found variant mapped to 404.
- If `thread_id` is absent, return a pending response with empty messages.
- If bound, load `ThreadRecord` by `thread_id`.
- Recover parent thread metadata with job manager if possible.
- If parent thread and parent session exist, verify user access by calling `get_thread_snapshot_for_user(user_id, parent_session_id, &parent_thread_id)`.
- Read job thread messages with `thread_repo.get_messages(&thread_id)` and map to `ChatMessage` using existing message conversion helpers in session manager if available. If no helper exists, add a small private converter mirroring current session repository mapping.
- Fill `turn_count`, `token_count`, and title from `ThreadRecord`.

**Step 5: Run tests**

Run:

```bash
cargo test -p argus-server --test chat_api thread_jobs_lists_dispatched_subagents_for_parent_thread get_chat_job_returns_readonly_conversation_messages get_chat_job_reports_pending_when_job_has_no_thread_binding
cargo test -p argus-server --test chat_api
```

Expected: PASS.

**Step 6: Commit**

```bash
git add crates/argus-server/src/server_core.rs crates/argus-server/src/routes/chat.rs crates/argus-server/tests/chat_api.rs
git commit -m "feat: add read-only chat job api"
```

### Task 4: Add Web API Client Types

**Files:**
- Modify: `apps/web/src/lib/api.ts`
- Modify: `apps/web/src/lib/api.test.ts`

**Step 1: Write failing client tests**

In `apps/web/src/lib/api.test.ts`, add tests that mock `fetch`:

```ts
it("lists jobs dispatched from a chat thread", async () => {
  fetchMock.mockResponseOnce(JSON.stringify([{ job_id: "job-1", title: "job:job-1" }]));
  const result = await new HttpApiClient().listChatThreadJobs("session-1", "thread-1");
  expect(fetchMock).toHaveBeenCalledWith(
    "/api/v1/chat/sessions/session-1/threads/thread-1/jobs",
    expect.any(Object),
  );
  expect(result[0]!.job_id).toBe("job-1");
});

it("gets a read-only chat job conversation", async () => {
  fetchMock.mockResponseOnce(JSON.stringify({ job_id: "job-1", messages: [] }));
  const result = await new HttpApiClient().getChatJobConversation("job-1");
  expect(fetchMock).toHaveBeenCalledWith("/api/v1/chat/jobs/job-1", expect.any(Object));
  expect(result.job_id).toBe("job-1");
});
```

Adjust instantiation to match the existing exported test pattern.

**Step 2: Run test to verify failure**

Run:

```bash
cd apps/web && pnpm exec vitest run src/lib/api.test.ts
```

Expected: FAIL because methods/types do not exist.

**Step 3: Add types and methods**

In `apps/web/src/lib/api.ts`, add:

```ts
export type ChatJobStatus = "pending" | "queued" | "running" | "succeeded" | "failed" | "cancelled";

export interface ChatThreadJobSummary {
  job_id: string;
  title: string;
  subagent_name: string;
  status: ChatJobStatus | string;
  created_at: string | null;
  updated_at: string | null;
  result_preview: string | null;
  bound_thread_id: string | null;
}

export interface ChatJobConversation {
  job_id: string;
  title: string;
  status: ChatJobStatus | string;
  thread_id: string | null;
  session_id: string | null;
  parent_session_id: string | null;
  parent_thread_id: string | null;
  messages: ChatMessageRecord[];
  turn_count: number;
  token_count: number;
  plan_item_count: number;
}
```

Extend `ApiClient`:

```ts
listChatThreadJobs?(sessionId: string, threadId: string): Promise<ChatThreadJobSummary[]>;
getChatJobConversation?(jobId: string): Promise<ChatJobConversation>;
subscribeChatJobConversation?(jobId: string, handlers: ChatThreadEventHandlers): RuntimeEventSubscription;
```

Implement in `HttpApiClient`:

```ts
listChatThreadJobs(sessionId: string, threadId: string): Promise<ChatThreadJobSummary[]> {
  return this.request(`/chat/sessions/${sessionId}/threads/${threadId}/jobs`);
}

getChatJobConversation(jobId: string): Promise<ChatJobConversation> {
  return this.request(`/chat/jobs/${jobId}`);
}
```

Leave `subscribeChatJobConversation` for Task 7 if server SSE is implemented then.

**Step 4: Run tests**

Run:

```bash
cd apps/web && pnpm exec vitest run src/lib/api.test.ts
```

Expected: PASS.

**Step 5: Commit**

```bash
git add apps/web/src/lib/api.ts apps/web/src/lib/api.test.ts
git commit -m "feat(web): add chat job api client"
```

### Task 5: Render Dispatched Jobs on `/chat`

**Files:**
- Create: `apps/web/src/features/chat/components/DispatchedJobsPanel.vue`
- Create: `apps/web/src/features/chat/components/DispatchedJobsPanel.test.ts`
- Modify: `apps/web/src/features/chat/ChatPage.vue`
- Modify: `apps/web/src/features/chat/chat-page.test.ts`

**Step 1: Write component tests**

Create `DispatchedJobsPanel.test.ts`:

```ts
it("renders empty state when no jobs were dispatched", () => {
  const wrapper = mount(DispatchedJobsPanel, { props: { jobs: [], loading: false, error: "" } });
  expect(wrapper.text()).toContain("暂无派发的 subagent");
});

it("emits openJob when clicking a dispatched job", async () => {
  const wrapper = mount(DispatchedJobsPanel, {
    props: {
      loading: false,
      error: "",
      jobs: [{
        job_id: "job-1",
        title: "job:job-1",
        subagent_name: "Researcher",
        status: "running",
        created_at: null,
        updated_at: null,
        result_preview: "正在分析",
        bound_thread_id: "thread-job",
      }],
    },
  });

  await wrapper.get("[data-testid='dispatched-job-job-1']").trigger("click");
  expect(wrapper.emitted("openJob")![0]).toEqual(["job-1"]);
});
```

**Step 2: Run to verify failure**

Run:

```bash
cd apps/web && pnpm exec vitest run src/features/chat/components/DispatchedJobsPanel.test.ts
```

Expected: FAIL because component does not exist.

**Step 3: Implement component**

Create `DispatchedJobsPanel.vue` with props:

```ts
const props = defineProps<{
  jobs: ChatThreadJobSummary[];
  loading: boolean;
  error: string;
}>();

const emit = defineEmits<{
  (e: "openJob", jobId: string): void;
  (e: "refresh"): void;
}>();
```

Render:

- Header: `已派发 subagent`
- Refresh icon/text button: `刷新`
- Loading state: `正在加载派发记录...`
- Error state: `派发记录加载失败，可刷新重试`
- Empty state: `暂无派发的 subagent`
- Job rows as buttons with `data-testid="dispatched-job-${job.job_id}"`

Use existing CSS tokens and keep radius at `var(--radius-md)` or less.

**Step 4: Integrate in ChatPage**

In `ChatPage.vue`:

- Import `useRouter` and `DispatchedJobsPanel`.
- Add refs:

```ts
const dispatchedJobs = ref<ChatThreadJobSummary[]>([]);
const dispatchedJobsLoading = ref(false);
const dispatchedJobsError = ref("");
```

- Add `refreshDispatchedJobs()` that calls `api.listChatThreadJobs!(activeSessionId, activeThreadId)`.
- Watch `activeSessionId/activeThreadId` and refresh when both exist; clear when no thread.
- Pass a callback into `useChatThreadStream` or add a watcher on runtime activities length/status to refresh after job events. Prefer extending `useChatThreadStream` with an optional `onJobEvent` callback in a later small edit if direct event access is cleaner.
- Add `openJob(jobId)`:

```ts
void router.push({
  name: "chat-job",
  params: { jobId },
  query: {
    fromSession: chatSessions.activeSessionId.value,
    fromThread: chatSessions.activeThreadId.value,
  },
});
```

Render the panel in the floating layer below `RuntimeActivityRail`.

**Step 5: Add ChatPage behavior tests**

In `chat-page.test.ts`, add:

```ts
it("loads dispatched subagent jobs for the active thread", async () => {
  const listChatThreadJobs = vi.fn().mockResolvedValue([
    { job_id: "job-1", title: "job:job-1", subagent_name: "Researcher", status: "running", created_at: null, updated_at: null, result_preview: null, bound_thread_id: "thread-job" },
  ]);
  setApiClient(makeApiClient({ listChatThreadJobs, listChatSessions: ..., listChatThreads: ... }));
  const wrapper = mount(ChatPage, { global: { plugins: [router] } });
  await flushPromises();
  expect(wrapper.text()).toContain("已派发 subagent");
  expect(wrapper.text()).toContain("Researcher");
});
```

Use existing test fixtures (`session()`, `thread()`, `makeApiClient`) from the file.

**Step 6: Run tests**

Run:

```bash
cd apps/web && pnpm exec vitest run src/features/chat/components/DispatchedJobsPanel.test.ts src/features/chat/chat-page.test.ts
```

Expected: PASS.

**Step 7: Commit**

```bash
git add apps/web/src/features/chat
git commit -m "feat(web): show dispatched subagents in chat"
```

### Task 6: Add Read-Only Job Chat Page

**Files:**
- Create: `apps/web/src/features/chat/JobChatPage.vue`
- Create: `apps/web/src/features/chat/job-chat-page.test.ts`
- Modify: `apps/web/src/router/index.ts`

**Step 1: Write route/page tests**

Create `job-chat-page.test.ts`:

```ts
it("renders a read-only job conversation with breadcrumb and no composer", async () => {
  setApiClient(makeApiClient({
    getChatJobConversation: vi.fn().mockResolvedValue({
      job_id: "job-1",
      title: "job:job-1",
      status: "succeeded",
      thread_id: "thread-job",
      session_id: "thread-job",
      parent_session_id: "session-1",
      parent_thread_id: "thread-1",
      messages: [message("assistant", "子任务完成")],
      turn_count: 1,
      token_count: 10,
      plan_item_count: 0,
    }),
  }));

  const router = createRouter({
    history: createMemoryHistory(),
    routes: [{ path: "/chat/jobs/:jobId", name: "chat-job", component: JobChatPage }],
  });
  await router.push("/chat/jobs/job-1");
  const wrapper = mount(JobChatPage, { global: { plugins: [router] } });
  await flushPromises();

  expect(wrapper.text()).toContain("对话");
  expect(wrapper.text()).toContain("Job job-1");
  expect(wrapper.text()).toContain("子任务完成");
  expect(wrapper.find("[data-testid='chat-input']").exists()).toBe(false);
});
```

Add a second test for pending/no thread:

```ts
it("shows pending binding state for a job without execution thread", async () => {
  // getChatJobConversation returns thread_id: null, messages: []
  // Assert text contains "执行线程尚未就绪".
});
```

**Step 2: Run to verify failure**

Run:

```bash
cd apps/web && pnpm exec vitest run src/features/chat/job-chat-page.test.ts
```

Expected: FAIL because page/route do not exist.

**Step 3: Add route**

In `apps/web/src/router/index.ts`:

```ts
const JobChatPage = () => import("@/features/chat/JobChatPage.vue");
```

Add child route after `/chat`:

```ts
{
  path: "chat/jobs/:jobId",
  name: "chat-job",
  component: JobChatPage,
  meta: { breadcrumb: "Job 对话", immersive: true, hideRouteHeader: true },
}
```

Update `AdminLayout` role guard if needed so non-admin chat users can access paths starting with `/chat`, not only exactly `/chat`:

```ts
if (!isAdminUser.value && !route.path.startsWith("/chat")) {
  void router.replace("/chat");
}
```

**Step 4: Implement JobChatPage**

Create `JobChatPage.vue`:

- Use `useRoute`, `useRouter`, `computed`, `onMounted`, `ref`.
- Load `getChatJobConversation(jobId)`.
- Convert `conversation.messages` through `toRobotMessages({ streaming: false, hasActiveThread: Boolean(thread_id), ... })`.
- Render top bar:
  - button `返回父对话`
  - breadcrumb text `对话 / Job ${jobId}`
- Render `ChatConversationPanel` for messages.
- If `thread_id` is null, render notice `Job 已创建，执行线程尚未就绪。`
- No `ChatComposerBar`.

Return behavior:

```ts
function returnToParent() {
  const session = conversation.value?.parent_session_id ?? route.query.fromSession;
  const thread = conversation.value?.parent_thread_id ?? route.query.fromThread;
  if (typeof session === "string" && typeof thread === "string") {
    void router.push({ name: "chat", query: { session, thread } });
    return;
  }
  void router.push({ name: "chat" });
}
```

If current `/chat` does not yet consume `session/thread` query, leave the query for Task 8 or push plain `/chat`.

**Step 5: Run tests**

Run:

```bash
cd apps/web && pnpm exec vitest run src/features/chat/job-chat-page.test.ts src/router/index.ts
```

If router test command is not valid, run:

```bash
cd apps/web && pnpm exec vitest run src/features/chat/job-chat-page.test.ts src/app/admin-console.smoke.test.ts
```

Expected: PASS.

**Step 6: Commit**

```bash
git add apps/web/src/router/index.ts apps/web/src/layouts/AdminLayout.vue apps/web/src/features/chat/JobChatPage.vue apps/web/src/features/chat/job-chat-page.test.ts
git commit -m "feat(web): add read-only job chat page"
```

### Task 7: Add Job Conversation Refresh and Optional SSE

**Files:**
- Modify: `crates/argus-server/src/routes/mod.rs`
- Modify: `crates/argus-server/src/routes/chat.rs`
- Modify: `crates/argus-server/src/server_core.rs`
- Modify: `apps/web/src/lib/api.ts`
- Modify: `apps/web/src/features/chat/JobChatPage.vue`
- Test: `apps/web/src/features/chat/job-chat-page.test.ts`

**Step 1: Start with polling if SSE is not ready**

Implement polling first. In `JobChatPage.vue`, if status is `pending`, `queued`, or `running`, start a timer that calls `getChatJobConversation(jobId)` every 1500 ms. Clear the timer on unmount or once status becomes terminal.

Add a test with fake timers:

```ts
it("polls while the job conversation is not terminal", async () => {
  vi.useFakeTimers();
  const getChatJobConversation = vi.fn()
    .mockResolvedValueOnce({ status: "running", messages: [] })
    .mockResolvedValueOnce({ status: "succeeded", messages: [message("assistant", "完成")] });
  // mount, advance timers, assert second call and rendered message.
});
```

**Step 2: Run test to verify failure**

Run:

```bash
cd apps/web && pnpm exec vitest run src/features/chat/job-chat-page.test.ts
```

Expected: FAIL until polling exists.

**Step 3: Implement polling**

Use `window.setInterval` and `window.clearInterval`, guarded for test cleanup.

**Step 4: Optional SSE route**

Only add `GET /api/v1/chat/jobs/:jobId/events` if implementation can reuse `SessionManager::subscribe_for_user` after resolving parent authorization. If added, mirror `thread_events` but resolve receiver through a new `ServerCore::subscribe_chat_job_conversation`.

Keep this separate from the minimum UX; polling is acceptable for first implementation.

**Step 5: Run tests**

Run:

```bash
cd apps/web && pnpm exec vitest run src/features/chat/job-chat-page.test.ts
cargo test -p argus-server --test chat_api
```

Expected: PASS.

**Step 6: Commit**

```bash
git add apps/web/src/features/chat/JobChatPage.vue apps/web/src/features/chat/job-chat-page.test.ts apps/web/src/lib/api.ts crates/argus-server/src/routes/mod.rs crates/argus-server/src/routes/chat.rs crates/argus-server/src/server_core.rs
git commit -m "feat(web): refresh read-only job conversations"
```

### Task 8: Parent Return and Query Activation

**Files:**
- Modify: `apps/web/src/features/chat/ChatPage.vue`
- Modify: `apps/web/src/features/chat/composables/useChatSessions.ts`
- Modify: `apps/web/src/features/chat/chat-page.test.ts`

**Step 1: Write failing return behavior test**

Add a test to `chat-page.test.ts`:

```ts
it("activates a requested session and thread from route query", async () => {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [{ path: "/chat", name: "chat", component: ChatPage }],
  });
  await router.push("/chat?session=session-2&thread=thread-2");
  // mock list sessions/threads for session-2.
  // mount with router.
  // assert listChatMessages called with session-2/thread-2.
});
```

**Step 2: Run to verify failure**

Run:

```bash
cd apps/web && pnpm exec vitest run src/features/chat/chat-page.test.ts
```

Expected: FAIL because `/chat` ignores query.

**Step 3: Implement query-aware initial selection**

In `ChatPage.vue`, import `useRoute`.

After `chatSessions.loadInitialState()`, read:

```ts
const requestedSessionId = typeof route.query.session === "string" ? route.query.session : "";
const requestedThreadId = typeof route.query.thread === "string" ? route.query.thread : "";
```

Prefer requested IDs when present:

- Load sessions.
- If requested session exists, `selectSession(requestedSessionId, requestedThreadId)`.
- Otherwise fall back to current first session behavior.

This may require changing `useChatSessions.loadInitialState(preferred?: { sessionId?: string; threadId?: string })`.

**Step 4: Run tests**

Run:

```bash
cd apps/web && pnpm exec vitest run src/features/chat/chat-page.test.ts src/features/chat/composables/useChatSessions.ts
```

If composable has no direct test command, run the relevant chat page test only.

Expected: PASS.

**Step 5: Commit**

```bash
git add apps/web/src/features/chat/ChatPage.vue apps/web/src/features/chat/composables/useChatSessions.ts apps/web/src/features/chat/chat-page.test.ts
git commit -m "feat(web): return to parent chat thread"
```

### Task 9: Full Verification

**Files:**
- No source changes expected unless tests reveal defects.

**Step 1: Run targeted server tests**

Run:

```bash
cargo test -p argus-job
cargo test -p argus-wing dispatched_jobs_for_thread_recovers_child_job_bindings
cargo test -p argus-server --test chat_api
```

Expected: PASS.

**Step 2: Run targeted Web tests**

Run:

```bash
cd apps/web && pnpm exec vitest run src/lib/api.test.ts src/features/chat/components/DispatchedJobsPanel.test.ts src/features/chat/job-chat-page.test.ts src/features/chat/chat-page.test.ts
```

Expected: PASS.

**Step 3: Run broader Web checks**

Run:

```bash
cd apps/web && pnpm exec vitest run
cd apps/web && pnpm build
```

Expected: PASS.

**Step 4: Run formatting and repo hooks**

Run:

```bash
prek
```

Expected: PASS.

**Step 5: Manual QA**

Run the Web app with the server fixture used locally:

```bash
cd apps/web && pnpm dev
```

Open `/chat`, dispatch a subagent through a scheduler-capable agent, verify:

- “已派发 subagent” appears after dispatch.
- Clicking a job opens `/chat/jobs/:jobId`.
- Job page shows breadcrumb and return button.
- Job page has no composer.
- Returning activates the parent chat thread.

**Step 6: Final commit if needed**

If verification changes were required:

```bash
git add <changed-files>
git commit -m "fix: polish web job conversation flow"
```

### Task 10: Implementation Notes and Guardrails

**Do not:**

- Add SQL outside `argus-repository`.
- Let Web call scheduler tool JSON actions directly.
- Add `POST /api/v1/chat/jobs/:jobId/messages`.
- Reuse desktop chat store in `apps/web`.
- Depend on front-end derivation of job runtime `session_id`.

**Prefer:**

- Repository traits or existing job manager recovery for all persistent lookup.
- Product-level DTOs in `ServerCore`.
- Read-only page tests that assert composer absence and API absence.
- Polling before SSE if SSE support would expand scope too much.
