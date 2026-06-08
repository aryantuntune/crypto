export type Role = 'user' | 'assistant' | 'system';

export interface Message {
  id: number;
  ts: number;
  role: Role;
  content: string;
  image_path?: string | null;
  prediction_id?: number | null;
}

export type Action = 'buy' | 'sell' | 'hold';
export type Horizon = '1h' | '4h' | '1d' | '3d' | '1w';

export interface Citation { doc: string; page?: number; }

export interface AnalysisJson {
  action: Action;
  probability_up: number;
  horizon: Horizon;
  stop_loss_pct?: number;
  take_profit_pct?: number;
  key_signals: string[];
  citations: Citation[];
}

export interface DailyCost {
  date: string;
  input_tokens: number;
  cached_input_tokens: number;
  output_tokens: number;
  cost_usd: number;
}

export interface Settings {
  daily_cost_cap_usd: number;
  model_main: string;
  model_extract: string;
  library_path: string | null;
  hotkey: string;
}

export interface DocInfo {
  doc_path: string;
  doc_type: string;
  chunks: number;
}
