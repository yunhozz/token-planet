export function LoadingStatus({ label, className = "" }: { label: string; className?: string }) {
  return (
    <div className={`loading-status ${className}`.trim()} role="status" aria-live="polite">
      <span className="loading-status-spinner" aria-hidden="true" />
      <span>{label}</span>
    </div>
  );
}
