import React from 'react';
// Modular subpath import avoids barrel import bloat
import Button from '@mui/material/Button';

export function ItemView({ items }: { items: { id: number; name: string }[] }) {
  return (
    <div>
      {items.map(item => (
        <span key={item.id}>{item.name}</span>
      ))}
      <Button>Action</Button>
    </div>
  );
}
