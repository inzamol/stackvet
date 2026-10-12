import React, { useState } from 'react';

export function HookIf({ shouldTrack }: { shouldTrack: boolean }) {
  // RCT-002: React Hook invoked conditionally inside if-statement
  if (shouldTrack) {
    useState(0);
  }
  return <div>Component</div>;
}
