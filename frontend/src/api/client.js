import axios from 'axios'

const client = axios.create({
  baseURL: '',
  timeout: 30000,
  withCredentials: true,
})

export function getApiErrorMessage(error, fallback = '操作失败') {
  if (error?.response?.status === 401) {
    return '请先登录后再继续'
  }
  return error?.response?.data?.detail || fallback
}

client.interceptors.response.use(
  (response) => response,
  (error) => {
    if (error.response?.status === 401) {
      window.dispatchEvent(new CustomEvent('auth:unauthorized'))
    }
    return Promise.reject(error)
  }
)

export default client
