import { ask } from "@tauri-apps/plugin-dialog";

export function confirmDialog(message: string): Promise<boolean> {
  return ask(message, { title: "Terminal Conductor", kind: "warning" }).catch(
    () => false,
  );
}
