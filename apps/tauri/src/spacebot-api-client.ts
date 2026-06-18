export interface WorkerListItem {
	id: string;
	name?: string | null;
	channel_id?: string | null;
	channel_name?: string | null;
	worker_type?: string | null;
	status?: string | null;
	task?: string | null;
	started_at?: string | null;
	completed_at?: string | null;
	has_transcript?: boolean | null;
	live_status?: string | null;
	tool_calls?: number | null;
	opencode_port?: number | null;
	opencode_session_id?: string | null;
	directory?: string | null;
	interactive?: boolean | null;
	project_id?: string | null;
	project_name?: string | null;
}

export interface TimelineItem {
	id: string;
	type: string;
	role?: string;
	content: string;
	started_at?: string;
	completed_at?: string;
	status?: string;
	task?: string;
}

export interface PortalConversationSummary {
	id: string;
	title: string;
	agent_id: string;
	created_at: string;
	updated_at: string;
	last_message_preview?: string | null;
}

export interface PortalHistoryMessage {
	id: string;
	role: string;
	content: string;
	created_at: string;
}

export interface PortalConversationResponse {
	conversation: PortalConversationSummary;
	history: PortalHistoryMessage[];
}

export interface TtsProfile {
	id: string;
	name: string;
}

export interface Task {
	id: string;
	task_number: number;
	title: string;
	description?: string | null;
	status: string;
	priority: string;
	agent_id?: string;
	owner_agent_id: string;
	assigned_agent_id: string;
	subtasks: Array<{ title: string; completed: boolean }>;
	metadata: unknown;
	worker_id?: string | null;
	created_by: string;
	created_at: string;
	updated_at: string;
	completed_at?: string | null;
}

export interface UpdateTaskRequest {
	id?: string;
	status?: string;
	priority?: string;
	complete_subtask?: number;
}

export interface InboundMessageEvent {
	type: "message";
	agent_id?: string;
	channel_id?: string;
	text: string;
	sender_id?: string | null;
	sender_name?: string | null;
	message?: PortalHistoryMessage;
}

export interface OutboundMessageDeltaEvent {
	type: "message_delta";
	agent_id?: string;
	channel_id?: string;
	delta?: string;
	aggregated_text: string;
}

export interface OutboundMessageEvent {
	type: "message_complete";
	agent_id?: string;
	channel_id?: string;
	text: string;
	message?: PortalHistoryMessage;
}

export interface TypingStateEvent {
	type: "typing";
	agent_id?: string;
	channel_id?: string;
	typing?: boolean;
	is_typing: boolean;
}

interface WorkerDetail {
	id: string;
	task: string;
	status: string;
	started_at: string;
	completed_at?: string | null;
	result?: string | null;
	transcript?: unknown[];
}

let serverUrl = "http://127.0.0.1:19898";

function unavailable(feature: string): Error {
	return new Error(`${feature} is unavailable because Spacebot is not bundled.`);
}

export function setServerUrl(url: string) {
	serverUrl = url;
}

export function getEventsUrl(agentId?: string, conversationId?: string): string {
	const url = new URL("/events", serverUrl);

	if (agentId) url.searchParams.set("agent_id", agentId);
	if (conversationId) url.searchParams.set("conversation_id", conversationId);

	return url.toString();
}

export const apiClient = {
	async channelMessages(
		_conversationId: string,
		_limit: number,
	): Promise<{ items: TimelineItem[] }> {
		return { items: [] };
	},

	async listWorkers(_options: {
		agentId: string;
		limit: number;
	}): Promise<{ workers: WorkerListItem[] }> {
		return { workers: [] };
	},

	async workerDetail(
		_agentId: string,
		_workerId: string,
	): Promise<WorkerDetail> {
		throw unavailable("Worker details");
	},

	async cancelProcess(_options: {
		agentId?: string;
		channelId?: string;
		processType?: string;
		processId: string;
	}): Promise<void> {},

	async listPortalConversations(
		_agentId: string,
		_activeOnly: boolean,
		_limit: number,
	): Promise<{
		conversations: PortalConversationSummary[];
	}> {
		return { conversations: [] };
	},

	async portalHistory(
		_agentId: string,
		_conversationId: string,
		_limit: number,
	): Promise<PortalHistoryMessage[]> {
		return [];
	},

	async createPortalConversation(_options: {
		agentId: string;
		title?: string | null;
	}): Promise<PortalConversationResponse> {
		throw unavailable("Spacebot conversations");
	},

	async portalSend(_options: {
		agentId: string;
		conversationId?: string;
		sessionId?: string;
		senderName?: string;
		message: string;
	}): Promise<void> {
		throw unavailable("Spacebot messaging");
	},

	async listTasks(_agentId: string, _limit: number): Promise<{ tasks: Task[] }> {
		return { tasks: [] };
	},

	async updateTask(
		_taskNumber: number,
		_req: UpdateTaskRequest,
	): Promise<unknown> {
		throw unavailable("Spacebot tasks");
	},

	async deleteTask(_taskNumber: number): Promise<unknown> {
		throw unavailable("Spacebot tasks");
	},

	async ttsGenerate(_text: string, _options: unknown): Promise<ArrayBuffer> {
		throw unavailable("Text to speech");
	},

	async webChatSendAudio(
		_agentId: string,
		_sessionId: string,
		_audioBlob: Blob,
	): Promise<{ ok: boolean; status: number }> {
		return { ok: false, status: 503 };
	},

	async ttsProfiles(_agentId: string): Promise<TtsProfile[]> {
		return [];
	},
};
