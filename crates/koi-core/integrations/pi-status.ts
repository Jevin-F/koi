// Loaded by Koi into pi sessions it starts: writes pi's activity to the file
// named by KOI_STATUS_FILE so Koi can show it.
import { writeFileSync } from "node:fs";

type StartEvent = { messageType?: string; prompt?: string };
type EventContext = { sessionManager: { getSessionFile(): string | undefined } };

export default function (pi: { on(event: string, handler: (event: StartEvent, ctx: EventContext) => void): void }) {
  const file = process.env.KOI_STATUS_FILE;
  if (!file) {
    return;
  }
  const report = (activity: string) => () => writeFileSync(file, activity);
  pi.on("agent_start", report("working"));
  pi.on("agent_settled", report("idle"));
  const titleFile = process.env.KOI_TITLE_FILE;
  const resumeFile = process.env.KOI_RESUME_FILE;
  if (titleFile || resumeFile) {
    pi.on("before_agent_start", (event, ctx) => {
      if (event.messageType !== "user" || !event.prompt) {
        return;
      }
      if (titleFile) {
        writeFileSync(titleFile, event.prompt.slice(0, 512));
      }
      const sessionFile = ctx.sessionManager.getSessionFile();
      if (resumeFile && sessionFile) {
        writeFileSync(resumeFile, sessionFile);
      }
    });
  }
}
