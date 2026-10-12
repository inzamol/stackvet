"use client";

import React from 'react';

// NXT-002: Private env variable accessed in Client Component
export function ClientComp() {
  const secretKey = process.env.DATABASE_PASSWORD;
  return <div>Loaded {secretKey ? "yes" : "no"}</div>;
}
