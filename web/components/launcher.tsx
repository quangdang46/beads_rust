"use client";
import * as React from "react";
import { useRouter } from "next/navigation";
import { FolderGit2, Loader2, Frown } from "lucide-react";
import { Icon } from "@/components/icons";
import { useTheme } from "@/components/theme-provider";
import { useProjects } from "@/hooks/use-projects";
import type { ProjectInfo } from "@/lib/api-client";

export function Launcher() {
  const { data, isLoading } = useProjects();
  const { mode, toggle } = useTheme();

  const projects = data?.projects ?? [];
  const real = projects.filter((p) => p.id !== "demo");

  // Auto-redirect if there's exactly one real project.
  const router = useRouter();
  React.useEffect(() => {
    if (real.length === 1) {
      router.replace(`/p/${real[0].id}`);
    }
  }, [real, router]);

  return (
    <div className="flex h-full flex-col overflow-y-auto bg-background text-foreground">
      <header className="flex items-center gap-[12px] border-b border-border p-[20px_28px]">
        <div
          className="flex h-[34px] w-[34px] items-center justify-center rounded-[10px] text-white"
          style={{ background: "var(--brand)", boxShadow: "0 2px 8px -2px var(--brand)" }}
        >
          <Icon name="logo" size={19} />
        </div>
        <div className="flex-1 leading-[1.15]">
          <div className="text-[16px] font-[680] tracking-[-.01em]">br — Issue Tracker</div>
          <div className="text-[12px] text-[var(--text-3)]">
            {real.length === 1 ? "Loading board..." : "No beads workspace found"}
          </div>
        </div>
        <button
          onClick={toggle}
          title="Toggle theme"
          className="flex h-[34px] w-[34px] items-center justify-center rounded-lg border border-border bg-[var(--surface)] text-[var(--text-2)] hover:bg-[var(--surface-2)]"
        >
          <Icon name={mode === "dark" ? "sun" : "moon"} size={15} />
        </button>
      </header>

      <div className="mx-auto w-full max-w-[860px] flex-1 p-[28px]">
        {isLoading ? (
          <div className="flex h-[200px] items-center justify-center text-[var(--text-3)]">
            <Loader2 size={20} className="animate-spin" />
          </div>
        ) : real.length === 0 ? (
          <div className="flex h-[200px] flex-col items-center justify-center gap-3 text-[var(--text-3)]">
            <Frown size={32} />
            <p className="text-[14px] font-medium">No beads workspace found.</p>
            <p className="text-[12px]">Run <code className="rounded bg-[var(--surface-2)] px-1.5 py-0.5 font-mono">br init</code> or launch <code className="rounded bg-[var(--surface-2)] px-1.5 py-0.5 font-mono">br web</code> from a project directory.</p>
          </div>
        ) : (
          <div className="grid grid-cols-[repeat(auto-fill,minmax(248px,1fr))] gap-[14px]">
            {real.map((p) => (
              <ProjectCard key={p.id} project={p} />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

function CardShell({
  onClick,
  children,
}: {
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <div
      role="button"
      tabIndex={0}
      onClick={onClick}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") onClick();
      }}
      className="group relative flex min-h-[112px] cursor-pointer flex-col gap-[10px] rounded-[13px] border border-border bg-[var(--surface)] p-[16px_18px] transition-colors hover:border-[var(--brand)] hover:bg-[var(--surface-2)]"
    >
      {children}
    </div>
  );
}

function ProjectCard({ project }: { project: ProjectInfo }) {
  const router = useRouter();
  return (
    <CardShell onClick={() => router.push(`/p/${project.id}`)}>
      <div className="flex items-center gap-[10px]">
        <div className="flex h-[30px] w-[30px] items-center justify-center rounded-[9px] bg-[var(--brand-weak)] text-[var(--brand)]">
          <FolderGit2 size={16} />
        </div>
        <div className="min-w-0 flex-1">
          <div className="truncate text-[14px] font-[620]">{project.name}</div>
          <div className="flex items-center gap-1.5 text-[11px] text-[var(--text-3)]">
            <span className="h-[6px] w-[6px] rounded-full bg-green-500" />
            beads workspace
          </div>
        </div>
      </div>
      <div className="truncate font-mono text-[11px] text-[var(--text-3)]" dir="rtl" title={project.path ?? ""}>
        {project.path}
      </div>
    </CardShell>
  );
}
