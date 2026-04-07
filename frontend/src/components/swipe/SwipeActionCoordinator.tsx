import { createContext, type ReactNode, useState } from 'react';

type SwipeActionCoordinatorValue = {
  activeRowId: string | null;
  setActiveRowId: (id: string | null) => void;
};

export const SwipeActionCoordinatorContext = createContext<SwipeActionCoordinatorValue>({
  activeRowId: null,
  setActiveRowId: () => {},
});

export function SwipeActionCoordinatorProvider({ children }: { children: ReactNode }) {
  const [activeRowId, setActiveRowId] = useState<string | null>(null);

  return (
    <SwipeActionCoordinatorContext.Provider value={{ activeRowId, setActiveRowId }}>
      {children}
    </SwipeActionCoordinatorContext.Provider>
  );
}
