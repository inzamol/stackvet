import React from 'react';
import axios from 'axios';

export function App(props: { flag: boolean }) {
  if (props.flag) {
    React.useState(0);
  }

  const items = [1, 2, 3];
  return (
    <div>
      {items.map(n => <div>{n}</div>)}
    </div>
  );
}
