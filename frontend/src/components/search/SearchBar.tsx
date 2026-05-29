import { useEffect, useRef } from 'react'
import { X } from 'lucide-react'
import { useChatStore } from '@/stores/chatStore'

export default function SearchBar() {
  const searchQuery = useChatStore((s) => s.searchQuery)
  const searchResults = useChatStore((s) => s.searchResults)
  const activeSearchIndex = useChatStore((s) => s.activeSearchIndex)
  const isSearchActive = useChatStore((s) => s.isSearchActive)
  const setSearchQuery = useChatStore((s) => s.setSearchQuery)
  const clearSearch = useChatStore((s) => s.clearSearch)
  const toggleSearch = useChatStore((s) => s.toggleSearch)
  const navigateSearch = useChatStore((s) => s.navigateSearch)
  const inputRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.ctrlKey && e.key === 'f') {
        e.preventDefault()
        toggleSearch()
      }
      if (e.key === 'Escape' && isSearchActive) {
        clearSearch()
      }
    }
    document.addEventListener('keydown', handleKeyDown)
    return () => document.removeEventListener('keydown', handleKeyDown)
  }, [isSearchActive, toggleSearch, clearSearch])

  useEffect(() => {
    if (isSearchActive) {
      inputRef.current?.focus()
    }
  }, [isSearchActive])

  if (!isSearchActive) return null

  const total = searchResults.length
  const current = total > 0 ? activeSearchIndex + 1 : 0

  return (
    <div className="flex items-center gap-2 px-3 py-1.5 bg-surface border-b border-border">
      <input
        ref={inputRef}
        type="text"
        value={searchQuery}
        onChange={(e) => setSearchQuery(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter') {
            e.preventDefault()
            navigateSearch(e.shiftKey ? 'prev' : 'next')
          }
          if (e.key === 'Escape') {
            clearSearch()
          }
        }}
        placeholder="搜索消息..."
        className="flex-1 bg-surface-light border border-border rounded px-2 py-1 text-sm text-text outline-none placeholder:text-text-darker focus:border-primary"
      />
      <span className="text-xs text-text-dim whitespace-nowrap font-mono">
        {current}/{total}
      </span>
      <button
        onClick={clearSearch}
        className="text-text-darker hover:text-text transition-colors"
      >
        <X size={14} />
      </button>
    </div>
  )
}