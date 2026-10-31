import { invoke } from '@tauri-apps/api/core';
export type User = { id: string; login: string; display_name: string; roles: string[]; active: boolean; mfa: boolean; scope: string };
export type Session = { token: string; user: User; permissions: string[] };
export type Status = { portable:boolean; first_run: boolean; version: string; database: string; storage: string; cipher: string; recovery: string | null };
export type AuditRow = { sequence: number; occurred_at: string; action: string; actor: string | null };
export type BackupSettings = { enabled: boolean; local_time: string; destination: string | null; last_success: string | null };
export type UpdateStatus = { configured: boolean; channel: string; phase: string; candidate: string | null; notes: string | null; last_check: string | null; message: string; manifest_verified: boolean; package_verified: boolean };
export type MfaSetup = { secret: string; uri: string };
export const native = '__TAURI_INTERNALS__' in window;
export function call<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
  if (!native) return Promise.reject(new Error('Cette interface doit être ouverte dans l’application Windows. Utilisez Lancer-Application.cmd.'));
  return invoke<T>(command, args);
}
export const message = (e: unknown) => e instanceof Error ? e.message : String(e);
