"use client";

import React, { useState } from 'react';

export function ClientCounter() {
  const [count, setCount] = useState(0);
  const apiUrl = process.env.NEXT_PUBLIC_API_URL;
  return (
    <div>
      <p>Count: {count}</p>
      <p>API: {apiUrl}</p>
      <button onClick={() => setCount(c => c + 1)}>Increment</button>
    </div>
  );
}
