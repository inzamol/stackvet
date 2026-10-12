import React from 'react';

export function List({ items }: { items: { id: number; name: string }[] }) {
  // RCT-004: JSX element in .map() loop is missing a unique 'key' prop
  return (
    <div>
      {items.map(item => (
        <span>{item.name}</span>
      ))}
    </div>
  );
}
