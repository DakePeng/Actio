import { useContext, useEffect, type KeyboardEvent, type ReactNode } from 'react';
import { SwipeActionCoordinatorContext } from './SwipeActionCoordinator';
import { useSwipeActionRow, type SwipePhase, type SwipeSide } from './useSwipeActionRow';

type SwipeActionConfig = {
  label: string;
  confirmLabel: string;
  onExecute: () => void | Promise<void>;
  destructive?: boolean;
};

export type SwipeActionRowProps = {
  rowId: string;
  leftAction?: SwipeActionConfig;
  rightAction?: SwipeActionConfig;
  disabled?: boolean;
  children: ReactNode;
};

function getRevealLabel(action: SwipeActionConfig, phase: SwipePhase) {
  if (phase === 'confirm') {
    return action.confirmLabel;
  }

  return action.label;
}

export function SwipeActionRow({
  rowId,
  leftAction,
  rightAction,
  disabled = false,
  children,
}: SwipeActionRowProps) {
  const { activeRowId, setActiveRowId } = useContext(SwipeActionCoordinatorContext);
  const { side, phase, reveal, close, confirmAction } = useSwipeActionRow();

  useEffect(() => {
    if (activeRowId !== null && activeRowId !== rowId && side !== null) {
      close();
    }
  }, [activeRowId, rowId, side, close]);

  const handleReveal = (target: Exclude<SwipeSide, null>) => {
    if (disabled) return;
    setActiveRowId(rowId);
    reveal(target);
  };

  const handleConfirm = async (target: Exclude<SwipeSide, null>, execute: () => void | Promise<void>) => {
    confirmAction(target);

    if (phase === 'confirm') {
      await execute();
      close();
      setActiveRowId(null);
    }
  };

  const handleKeyDown = async (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === 'Delete' && leftAction) {
      event.preventDefault();
      handleReveal('left');
      return;
    }

    if (event.key.toLowerCase() === 'e' && rightAction) {
      event.preventDefault();
      handleReveal('right');
      return;
    }

    if (event.key === 'Enter' && side === 'left' && leftAction) {
      event.preventDefault();
      await handleConfirm('left', leftAction.onExecute);
      return;
    }

    if (event.key === 'Enter' && side === 'right' && rightAction) {
      event.preventDefault();
      await handleConfirm('right', rightAction.onExecute);
      return;
    }

    if (event.key === 'Escape') {
      close();
      setActiveRowId(null);
    }
  };

  const isOpen = side !== null;

  return (
    <div
      className={`swipe-row${side ? ` is-${side}-open` : ''}${phase === 'confirm' ? ' is-confirming' : ''}`}
      tabIndex={0}
      onKeyDown={handleKeyDown}
    >
      <div className="swipe-row__actions swipe-row__actions--left">
        {side === 'left' && leftAction && (
          <button
            type="button"
            className={`swipe-row__action${leftAction.destructive ? ' is-destructive' : ''}`}
            aria-label={getRevealLabel(leftAction, phase)}
            onClick={() => handleConfirm('left', leftAction.onExecute)}
          >
            {getRevealLabel(leftAction, phase)}
          </button>
        )}
      </div>
      <div className="swipe-row__actions swipe-row__actions--right">
        {side === 'right' && rightAction && (
          <button
            type="button"
            className="swipe-row__action swipe-row__action--done"
            aria-label={getRevealLabel(rightAction, phase)}
            onClick={() => handleConfirm('right', rightAction.onExecute)}
          >
            {getRevealLabel(rightAction, phase)}
          </button>
        )}
      </div>
      <div className={`swipe-row__body${isOpen ? ' is-open' : ''}`}>
        {leftAction && (
          <button
            type="button"
            className="swipe-row__reveal swipe-row__reveal--left"
            aria-label="Reveal delete action"
            onClick={() => handleReveal('left')}
          />
        )}
        {rightAction && (
          <button
            type="button"
            className="swipe-row__reveal swipe-row__reveal--right"
            aria-label="Reveal edit action"
            onClick={() => handleReveal('right')}
          />
        )}
        {children}
      </div>
    </div>
  );
}
