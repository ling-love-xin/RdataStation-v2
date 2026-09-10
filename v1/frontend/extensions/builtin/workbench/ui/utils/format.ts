import { formatDate } from '@/shared/utils'

export function formatDateShort(dateStr: string | undefined): string {
  if (!dateStr) return '-'
  return formatDate(dateStr, 'YYYY-MM-DD')
}

export function formatDateTime(dateStr: string | undefined): string {
  if (!dateStr) return '-'
  return formatDate(dateStr, 'YYYY-MM-DD HH:mm:ss')
}
