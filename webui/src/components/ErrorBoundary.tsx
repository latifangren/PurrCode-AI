import React from 'react';
import { RotateCcw } from 'lucide-react';

interface ErrorBoundaryProps {
  children: React.ReactNode;
  resetKey?: string;
}

interface ErrorBoundaryState {
  hasError: boolean;
  message: string;
}

export class ErrorBoundary extends React.Component<
  ErrorBoundaryProps,
  ErrorBoundaryState
> {
  constructor(props: ErrorBoundaryProps) {
    super(props);
    this.state = { hasError: false, message: '' };
  }

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return {
      hasError: true,
      message: error.message || 'An unexpected rendering error occurred.',
    };
  }

  componentDidUpdate(prevProps: ErrorBoundaryProps) {
    if (prevProps.resetKey !== this.props.resetKey && this.state.hasError) {
      this.setState({ hasError: false, message: '' });
    }
  }

  render() {
    if (this.state.hasError) {
      return (
        <div className="flex-1 flex items-center justify-center p-6">
          <div className="max-w-md w-full rounded-xl border border-hairline bg-surface-card p-6 space-y-3">
            <h2 className="font-editorial text-2xl text-ink">View Recovery</h2>
            <p className="text-sm text-body leading-relaxed">
              {this.state.message}
            </p>
            <button
              type="button"
              onClick={() => this.setState({ hasError: false, message: '' })}
              className="h-9 px-3.5 inline-flex items-center gap-1.5 rounded-md bg-primary hover:bg-primary-active text-on-primary text-xs font-medium transition-colors cursor-pointer"
            >
              <RotateCcw className="w-3.5 h-3.5" />
              <span>Retry View</span>
            </button>
          </div>
        </div>
      );
    }

    return this.props.children;
  }
}
