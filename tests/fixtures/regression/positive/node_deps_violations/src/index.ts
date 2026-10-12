// NDEP-001: axios is imported but not declared in dependencies
// NDEP-002: lodash is declared in dependencies but never imported
import axios from 'axios';

export async function loadData() {
  return axios.get('/api');
}
