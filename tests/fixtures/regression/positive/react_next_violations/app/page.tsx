// App router component without "use client"
import React, { useState } from 'react';

// NXT-001: Client hook used in Server Component without 'use client'
export default function Page() {
  const [count, setCount] = useState(0);
  return <div>{count}</div>;
}
