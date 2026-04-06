import client from './client'

export function fetchMe() {
  return client.get('/api/me')
}

export function logout() {
  return client.post('/auth/logout')
}
