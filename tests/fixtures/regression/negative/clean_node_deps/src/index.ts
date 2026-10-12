// All imported packages are declared in dependencies
import React from 'react';
import axios from 'axios';

export async function fetchUsers() {
  const res = await axios.get('/users');
  return res.data;
}
