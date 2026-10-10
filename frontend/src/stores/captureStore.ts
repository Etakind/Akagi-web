import { create } from 'zustand'
import type { CaptureStatus, GameRecoveryStatus } from '@/types'

type CaptureStore = {
  status: CaptureStatus
  set: (s: CaptureStatus) => void
  recovery: GameRecoveryStatus
  setRecovery: (s: GameRecoveryStatus) => void
}

export const useCaptureStore = create<CaptureStore>((set) => ({
  status: { state: 'stopped' },
  set: (status) => set({ status }),
  recovery: { phase: 'inactive', method: null, reason: null, epoch: 0, can_recover: false },
  setRecovery: (recovery) => set({ recovery }),
}))
