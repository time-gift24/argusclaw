import { ref, computed } from 'vue'
import { defineStore } from 'pinia'
import { fetchMe, logout as apiLogout } from '../api/auth'

export const useUserStore = defineStore('user', () => {
  const profile = ref(null)
  const loading = ref(true)

  const isLoggedIn = computed(() => !!profile.value)
  const userName = computed(() => profile.value?.display_name || '')
  const userId = computed(() => profile.value?.id || '')
  const account = computed(() => profile.value?.account || '')

  async function checkAuth() {
    loading.value = true
    try {
      const { data } = await fetchMe()
      profile.value = data
    } catch {
      profile.value = null
    } finally {
      loading.value = false
    }
  }

  async function logout() {
    try {
      await apiLogout()
    } finally {
      profile.value = null
      window.location.href = '/'
    }
  }

  function login() {
    window.location.href = '/auth/login'
  }

  return { profile, loading, isLoggedIn, userName, userId, account, checkAuth, logout, login }
})
