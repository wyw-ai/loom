import { Component, type ErrorInfo, type ReactNode } from "react";
import { useI18n } from "@/lib/i18n";

interface Props {
  children: ReactNode;
  fallback?: ReactNode;
}

interface State {
  error: Error | null;
  hasError: boolean;
  componentStack: string | null;
}

export class ErrorBoundary extends Component<Props, State> {
  constructor(props: Props) {
    super(props);
    this.state = { error: null, hasError: false, componentStack: null };
  }

  static getDerivedStateFromError(error: Error): Partial<State> {
    return { error, hasError: true };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    this.setState({ componentStack: info.componentStack ?? null });
    console.error("[ErrorBoundary] Caught render error:", error, info.componentStack);
  }

  render() {
    if (this.state.hasError) {
      if (this.props.fallback) return this.props.fallback;
      return (
        <DefaultErrorFallback
          error={this.state.error}
          componentStack={this.state.componentStack}
        />
      );
    }
    return this.props.children;
  }
}

function DefaultErrorFallback({
  componentStack,
  error,
}: {
  componentStack: string | null;
  error: Error | null;
}) {
  const { t } = useI18n();
  return (
    <div className="flex h-full w-full items-center justify-center bg-white p-8">
      <div className="max-w-md text-center">
        <h2 className="mb-2 text-lg font-semibold text-[#1a1a2e]">
          {t("Something went wrong")}
        </h2>
        <p className="mb-4 text-sm text-[#667085]">
          {t("An unexpected error occurred. Please try reloading the app.")}
        </p>
        <pre className="mb-4 max-h-40 overflow-auto rounded-md bg-[#f7f8fb] p-3 text-left text-xs text-[#485063]">
          {error?.message ?? t("Unknown error")}
        </pre>
        {componentStack && (
          <details className="mb-4">
            <summary className="cursor-pointer text-xs text-[#667085] hover:text-[#1a1a2e]">
              {t("Component Stack")}
            </summary>
            <pre className="mt-2 max-h-60 overflow-auto whitespace-pre-wrap rounded-md bg-[#f7f8fb] p-3 text-left text-[10px] text-[#485063]">
              {componentStack}
            </pre>
          </details>
        )}
        <button
          type="button"
          onClick={() => window.location.reload()}
          className="rounded-md bg-[#503ed4] px-4 py-2 text-sm font-medium text-white hover:bg-[#4535c4]"
        >
          {t("Reload")}
        </button>
      </div>
    </div>
  );
}
