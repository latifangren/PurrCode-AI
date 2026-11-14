import React, { useState } from 'react';
import { Check, Plus, Save } from 'lucide-react';
import type { MemoryFile, SkillEntry } from '../types/cockpit';
import { DiffViewer } from './DiffViewer';

interface MemoryViewProps {
  memoryFiles: MemoryFile[];
  skills: SkillEntry[];
  onSaveMemory: (name: MemoryFile['name'], content: string) => Promise<void>;
  onManageSkill: (payload: {
    action: 'toggle' | 'apply_diff' | 'create';
    skillId?: string;
    name?: string;
    description?: string;
  }) => Promise<void>;
}

export function MemoryView({
  memoryFiles,
  skills,
  onSaveMemory,
  onManageSkill,
}: MemoryViewProps) {
  const [selectedName, setSelectedName] = useState<MemoryFile['name']>('MEMORY.md');
  const currentFile = memoryFiles.find((f) => f.name === selectedName) ?? memoryFiles[0];

  const [drafts, setDrafts] = useState<Record<string, string>>(() => {
    const map: Record<string, string> = {};
    for (const f of memoryFiles) map[f.name] = f.content;
    return map;
  });
  const [saving, setSaving] = useState(false);
  const [savedFlag, setSavedFlag] = useState(false);

  const [skillName, setSkillName] = useState('');
  const [skillDesc, setSkillDesc] = useState('');

  const activeDraft = drafts[currentFile.name] ?? currentFile.content;

  const handleSave = async () => {
    setSaving(true);
    await onSaveMemory(currentFile.name, activeDraft);
    setSaving(false);
    setSavedFlag(true);
    setTimeout(() => setSavedFlag(false), 2000);
  };

  const handleCreateSkill = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!skillName.trim()) return;
    await onManageSkill({
      action: 'create',
      name: skillName.trim(),
      description: skillDesc.trim(),
    });
    setSkillName('');
    setSkillDesc('');
  };

  return (
    <div className="flex-1 overflow-y-auto px-4 sm:px-8 py-8">
      <div className="max-w-5xl mx-auto space-y-10">
        {/* Identity & Memory Markdown Studio */}
        <section className="space-y-5">
          <header className="flex flex-wrap items-end justify-between gap-4 pb-5 border-b border-hairline">
            <div className="space-y-1.5">
              <h1 className="font-editorial text-4xl text-ink">
                Identity, Memory & Skills Studio
              </h1>
              <p className="text-sm text-body max-w-2xl leading-relaxed">
                Edit persistent agent context files and manage procedural skills synthesized by Tinker.
              </p>
            </div>

            <button
              type="button"
              disabled={saving}
              onClick={handleSave}
              className="h-10 px-5 inline-flex items-center gap-2 rounded-md bg-primary hover:bg-primary-active text-on-primary text-sm font-medium transition-colors whitespace-nowrap shrink-0 cursor-pointer"
            >
              {savedFlag ? <Check className="w-4 h-4" /> : <Save className="w-4 h-4" />}
              <span>{saving ? 'Saving...' : savedFlag ? 'Saved' : `Save ${currentFile.name}`}</span>
            </button>
          </header>

          {/* File Selector Tabs */}
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div className="inline-flex rounded-md border border-hairline bg-surface-soft p-1">
              {memoryFiles.map((file) => (
                <button
                  key={file.name}
                  type="button"
                  onClick={() => {
                    setSelectedName(file.name);
                    setSavedFlag(false);
                  }}
                  className={`px-3.5 py-1.5 rounded font-mono text-xs font-medium transition-colors cursor-pointer ${
                    currentFile.name === file.name
                      ? 'bg-canvas text-ink'
                      : 'text-muted hover:text-ink'
                  }`}
                >
                  {file.name}
                </button>
              ))}
            </div>
            <span className="text-xs text-muted">
              {currentFile.description} · Updated {currentFile.updatedAt}
            </span>
          </div>

          {/* Dual-Pane Markdown Editor & Preview */}
          <div className="grid grid-cols-1 lg:grid-cols-2 gap-5">
            <div className="rounded-xl border border-code-border bg-code-bg overflow-hidden flex flex-col">
              <div className="px-4 py-2.5 bg-code-elevated flex items-center justify-between text-xs text-code-muted font-mono">
                <span>Source · {currentFile.name}</span>
                <span className="tabular-nums">{activeDraft.split('\n').length} lines</span>
              </div>
              <textarea
                aria-label={`Markdown editor for ${currentFile.name}`}
                value={activeDraft}
                onChange={(e) =>
                  setDrafts((prev) => ({ ...prev, [currentFile.name]: e.target.value }))
                }
                rows={14}
                className="w-full flex-1 bg-code-soft text-code-text p-4 font-mono text-[13px] leading-relaxed focus:outline-none resize-y dark-scroll"
              />
            </div>

            <div className="rounded-xl border border-hairline bg-surface-card overflow-hidden flex flex-col">
              <div className="px-4 py-2.5 border-b border-hairline bg-surface-soft text-xs font-medium text-muted">
                Rendered Context Preview
              </div>
              <div className="p-5 space-y-2.5 text-sm text-body-strong leading-relaxed overflow-y-auto max-h-[380px]">
                {activeDraft.split('\n').map((line, i) => {
                  if (line.startsWith('# ')) {
                    return (
                      <h3 key={i} className="font-editorial text-2xl text-ink pt-1">
                        {line.slice(2)}
                      </h3>
                    );
                  }
                  if (line.startsWith('## ')) {
                    return (
                      <h4 key={i} className="text-sm font-medium text-ink pt-2">
                        {line.slice(3)}
                      </h4>
                    );
                  }
                  if (line.startsWith('- ') || /^\d+\./.test(line)) {
                    return (
                      <p key={i} className="pl-3 text-[13px] text-body">
                        {line}
                      </p>
                    );
                  }
                  if (!line.trim()) return <div key={i} className="h-1" />;
                  return (
                    <p key={i} className="text-[13px] text-body">
                      {line}
                    </p>
                  );
                })}
              </div>
            </div>
          </div>
        </section>

        {/* Skills Catalog & Synthesized Diffs */}
        <section className="space-y-5 pt-4 border-t border-hairline">
          <div className="flex flex-wrap items-end justify-between gap-4">
            <div className="space-y-1">
              <h2 className="font-editorial text-3xl text-ink">
                Procedural Skills Catalog
              </h2>
              <p className="text-xs text-muted">
                Manage bundled and project-synthesized skills (<code className="font-mono">manage_skill</code>).
              </p>
            </div>
          </div>

          <form
            onSubmit={handleCreateSkill}
            className="rounded-xl border border-hairline bg-canvas p-4 flex flex-wrap items-center gap-3"
          >
            <input
              type="text"
              value={skillName}
              onChange={(e) => setSkillName(e.target.value)}
              placeholder="New skill slug (e.g. sql-migration-check)"
              aria-label="New skill name"
              className="h-10 flex-1 min-w-[200px] rounded-md border border-hairline bg-canvas px-3 font-mono text-xs text-ink focus:outline-none focus:border-primary"
            />
            <input
              type="text"
              value={skillDesc}
              onChange={(e) => setSkillDesc(e.target.value)}
              placeholder="Short capability description..."
              aria-label="New skill description"
              className="h-10 flex-[1.5] min-w-[220px] rounded-md border border-hairline bg-canvas px-3 text-xs text-ink focus:outline-none focus:border-primary"
            />
            <button
              type="submit"
              className="h-10 px-4 inline-flex items-center gap-1.5 rounded-md bg-primary hover:bg-primary-active text-on-primary text-xs font-medium transition-colors whitespace-nowrap cursor-pointer"
            >
              <Plus className="w-3.5 h-3.5" />
              <span>Create Skill</span>
            </button>
          </form>

          <div className="space-y-4">
            {skills.map((skill) => (
              <div
                key={skill.id}
                className="rounded-xl border border-hairline bg-surface-card p-5 space-y-4"
              >
                <div className="flex flex-wrap items-start justify-between gap-3">
                  <div className="space-y-1">
                    <div className="flex items-center gap-2">
                      <span className="font-mono text-sm font-medium text-ink">
                        {skill.name}
                      </span>
                      <span className="text-xs text-muted">
                        · {skill.scope} · <code className="font-mono">{skill.trigger}</code>
                      </span>
                    </div>
                    <p className="text-xs text-body">{skill.description}</p>
                  </div>

                  <button
                    type="button"
                    onClick={() => onManageSkill({ action: 'toggle', skillId: skill.id })}
                    className={`h-8 px-3 rounded-md text-xs font-medium transition-colors cursor-pointer ${
                      skill.enabled
                        ? 'bg-code-bg text-code-text'
                        : 'border border-hairline bg-canvas text-muted'
                    }`}
                  >
                    {skill.enabled ? 'Active' : 'Disabled'}
                  </button>
                </div>

                {skill.diffPreview && (
                  <div className="space-y-2.5">
                    <DiffViewer diff={skill.diffPreview} />
                    {skill.diffPreview.status === 'pending' && (
                      <div>
                        <button
                          type="button"
                          onClick={() =>
                            onManageSkill({ action: 'apply_diff', skillId: skill.id })
                          }
                          className="h-9 px-4 rounded-md bg-primary hover:bg-primary-active text-on-primary text-xs font-medium transition-colors cursor-pointer"
                        >
                          Accept Synthesized Skill Update
                        </button>
                      </div>
                    )}
                  </div>
                )}
              </div>
            ))}
          </div>
        </section>
      </div>
    </div>
  );
}
