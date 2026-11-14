import React, { useState } from 'react';
import { Plus, Trash2 } from 'lucide-react';
import { parseNaturalLanguageCron } from '../lib/cockpitLogic';
import type { AgentRole, CronJob } from '../types/cockpit';

interface CronViewProps {
  jobs: CronJob[];
  onAddJob: (naturalPrompt: string, targetAgent: AgentRole) => Promise<string | null>;
  onToggleJob: (id: string) => Promise<void>;
  onDeleteJob: (id: string) => Promise<void>;
}

const AGENTS: AgentRole[] = ['Tuxeshadow', 'Sphinx', 'Whisker', 'Meowstro', 'Tinker'];

export function CronView({ jobs, onAddJob, onToggleJob, onDeleteJob }: CronViewProps) {
  const [naturalInput, setNaturalInput] = useState('every 30 minutes');
  const [targetAgent, setTargetAgent] = useState<AgentRole>('Tuxeshadow');
  const [submitError, setSubmitError] = useState<string | null>(null);

  const preview = parseNaturalLanguageCron(naturalInput);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    const err = await onAddJob(naturalInput, targetAgent);
    if (err) {
      setSubmitError(err);
    } else {
      setSubmitError(null);
      setNaturalInput('');
    }
  };

  return (
    <div className="flex-1 overflow-y-auto px-4 sm:px-8 py-8">
      <div className="max-w-5xl mx-auto space-y-8">
        <header className="pb-5 border-b border-hairline space-y-1.5">
          <h1 className="font-editorial text-4xl text-ink">
            Natural Language Cron Scheduler
          </h1>
          <p className="text-sm text-body max-w-2xl leading-relaxed">
            Schedule recurring subagent audits and verification runs using plain-English intervals with instant expression previews.
          </p>
        </header>

        {/* NL Schedule Builder */}
        <form
          onSubmit={handleSubmit}
          className="rounded-xl border border-hairline bg-surface-card p-6 space-y-4"
        >
          <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
            <label className="md:col-span-2 space-y-1.5 text-xs font-medium text-body">
              <span>Schedule Phrase or 5-Field Cron</span>
              <input
                type="text"
                value={naturalInput}
                onChange={(e) => setNaturalInput(e.target.value)}
                placeholder='e.g. "every 15 minutes", "weekdays at 8:30am", "daily at 21:00"'
                className="h-10 w-full rounded-md border border-hairline bg-canvas px-3 text-sm text-ink focus:outline-none focus:border-primary"
              />
            </label>

            <label className="space-y-1.5 text-xs font-medium text-body">
              <span>Assigned Subagent</span>
              <select
                value={targetAgent}
                onChange={(e) => setTargetAgent(e.target.value as AgentRole)}
                className="h-10 w-full rounded-md border border-hairline bg-canvas px-3 text-sm text-ink focus:outline-none focus:border-primary"
              >
                {AGENTS.map((a) => (
                  <option key={a} value={a}>
                    {a}
                  </option>
                ))}
              </select>
            </label>
          </div>

          {/* Instant Preview Strip */}
          <div className="rounded-lg bg-code-bg text-code-text px-4 py-3 flex flex-wrap items-center justify-between gap-3 text-xs">
            {preview.valid ? (
              <>
                <div className="flex items-center gap-3">
                  <span className="font-mono text-primary font-medium">
                    {preview.expression}
                  </span>
                  <span className="text-code-text/85">{preview.summary}</span>
                </div>
                <span className="font-mono text-code-muted">Next: {preview.nextRun}</span>
              </>
            ) : (
              <span className="text-code-muted">{preview.error}</span>
            )}
          </div>

          {submitError && (
            <div role="alert" className="text-xs text-error font-medium">
              {submitError}
            </div>
          )}

          <div>
            <button
              type="submit"
              disabled={!preview.valid}
              className="h-10 px-5 inline-flex items-center gap-2 rounded-md bg-primary hover:bg-primary-active disabled:bg-primary-disabled disabled:text-muted text-on-primary text-sm font-medium transition-colors cursor-pointer"
            >
              <Plus className="w-4 h-4" />
              <span>Create Schedule</span>
            </button>
          </div>
        </form>

        {/* Active Cron Jobs Table */}
        <section className="rounded-xl border border-hairline bg-canvas overflow-hidden">
          <div className="px-5 py-3.5 border-b border-hairline bg-surface-soft text-xs font-medium text-muted">
            Scheduled Automation Jobs ({jobs.length})
          </div>
          <div className="divide-y divide-hairline-soft">
            {jobs.length === 0 ? (
              <div className="p-8 text-center text-sm text-muted">
                No scheduled jobs configured yet.
              </div>
            ) : (
              jobs.map((job) => (
                <div
                  key={job.id}
                  className="px-5 py-4 flex flex-wrap items-center justify-between gap-4 text-sm"
                >
                  <div className="space-y-1 min-w-0">
                    <div className="flex items-center gap-2.5 flex-wrap">
                      <span className="font-mono text-xs font-medium text-primary">
                        {job.expression}
                      </span>
                      <span className="font-medium text-ink">{job.summary}</span>
                    </div>
                    <div className="text-xs text-muted">
                      Agent: {job.targetAgent} · Next run: {job.nextRun}
                    </div>
                  </div>

                  <div className="flex items-center gap-2.5 shrink-0">
                    <button
                      type="button"
                      onClick={() => onToggleJob(job.id)}
                      className={`h-8 px-3 rounded-md text-xs font-medium transition-colors cursor-pointer ${
                        job.enabled
                          ? 'bg-code-bg text-code-text'
                          : 'border border-hairline bg-canvas text-muted'
                      }`}
                    >
                      {job.enabled ? 'Active' : 'Paused'}
                    </button>
                    <button
                      type="button"
                      onClick={() => onDeleteJob(job.id)}
                      aria-label={`Delete schedule ${job.summary}`}
                      className="h-8 w-8 inline-flex items-center justify-center rounded-md border border-hairline bg-canvas text-muted hover:text-error cursor-pointer"
                    >
                      <Trash2 className="w-3.5 h-3.5" />
                    </button>
                  </div>
                </div>
              ))
            )}
          </div>
        </section>
      </div>
    </div>
  );
}
