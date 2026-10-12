import React from 'react';
// RCT-005: Import from top-level barrel file causes bundle bloat
import { Button } from '@mui/material';

export function BarrelDemo() {
  return <Button>Click</Button>;
}
