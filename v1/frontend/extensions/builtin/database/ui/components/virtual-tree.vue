<template>
  <div
    ref="containerRef"
    class="virtual-tree-container"
    role="tree"
    aria-label="数据库导航树"
    tabindex="0"
    @scroll="onScroll"
    @keydown="onKeydown"
  >
    <div class="virtual-spacer" :style="{ height: `${totalHeight}px` }">
      <div class="virtual-content" :style="{ transform: `translateY(${offsetY}px)` }">
        <VirtualTreeNode
          v-for="node in visibleNodes"
          :key="node.key"
          :node="node"
          :is-selected="node.key === selectedKey"
          @expand="handleExpand"
          @select="handleSelect"
          @context-menu="handleContextMenu"
          @dblclick="handleDblClick"
        />
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch } from 'vue'

import VirtualTreeNode from './virtual-tree-node.vue'
import { useKeyboardNavigation } from '../composables/use-keyboard-navigation'
import { useVirtualScroll } from '../composables/use-virtual-scroll'

import type { VirtualTreeNode as VirtualTreeNodeType } from '../types/virtual-tree'

interface Props {
  /** 扁平化的节点数组 */
  nodes: VirtualTreeNodeType[]
  /** 每项高度 */
  itemHeight?: number
  /** 选中的节点 key */
  selectedKey: string | null
}

const props = withDefaults(defineProps<Props>(), {
  itemHeight: 28,
})

const emit = defineEmits<{
  expand: [node: VirtualTreeNodeType]
  select: [node: VirtualTreeNodeType]
  'context-menu': [node: VirtualTreeNodeType, event: MouseEvent]
  dblclick: [node: VirtualTreeNodeType]
  scroll: [scrollTop: number]
  toggle: [node: VirtualTreeNodeType]
}>()

const containerRef = ref<HTMLElement | null>(null)
const scrollTop = ref(0)
const containerHeight = ref(600)
const selectedKeyRef = ref<string | null>(props.selectedKey)
let scrollRafId: ReturnType<typeof requestAnimationFrame> | null = null

// 同步外部 selectedKey
watch(
  () => props.selectedKey,
  newKey => {
    selectedKeyRef.value = newKey
  }
)

// 虚拟滚动计算
const { totalHeight, visibleStart, visibleEnd, offsetY } = useVirtualScroll({
  containerHeight,
  scrollTop,
  itemHeight: props.itemHeight,
  totalItems: computed(() => props.nodes.length),
  bufferSize: 5,
})

// 可见节点
const visibleNodes = computed(() => props.nodes.slice(visibleStart.value, visibleEnd.value))

// 键盘导航
const { handleKeydown } = useKeyboardNavigation({
  nodes: computed(() => props.nodes),
  selectedKey: selectedKeyRef,
  onSelect: node => emit('select', node),
  onToggle: node => emit('toggle', node),
})

function onScroll() {
  // RAF 节流 — 避免高频 scroll 事件触发密集的 flatNodes 重算
  if (scrollRafId !== null) return
  scrollRafId = requestAnimationFrame(() => {
    scrollRafId = null
    if (containerRef.value) {
      scrollTop.value = containerRef.value.scrollTop
      emit('scroll', scrollTop.value)
    }
  })
}

function onKeydown(event: KeyboardEvent) {
  handleKeydown(event)
}

function handleExpand(node: VirtualTreeNodeType) {
  emit('toggle', node)
}

function handleSelect(node: VirtualTreeNodeType) {
  emit('select', node)
}

function handleContextMenu(node: VirtualTreeNodeType, event: MouseEvent) {
  emit('context-menu', node, event)
}

function handleDblClick(node: VirtualTreeNodeType) {
  emit('dblclick', node)
}

function updateContainerHeight() {
  if (containerRef.value) {
    containerHeight.value = containerRef.value.clientHeight
  }
}

onMounted(() => {
  updateContainerHeight()
  window.addEventListener('resize', updateContainerHeight)
})

onUnmounted(() => {
  if (scrollRafId !== null) {
    cancelAnimationFrame(scrollRafId)
    scrollRafId = null
  }
  window.removeEventListener('resize', updateContainerHeight)
})

defineExpose({
  scrollTo: (top: number) => {
    if (containerRef.value) {
      containerRef.value.scrollTop = top
    }
  },
  scrollToNode: (nodeKey: string) => {
    const index = props.nodes.findIndex(n => n.key === nodeKey)
    if (index !== -1 && containerRef.value) {
      containerRef.value.scrollTop = index * props.itemHeight - containerHeight.value / 2
    }
  },
})
</script>

<style scoped>
.virtual-tree-container {
  overflow-y: auto;
  overflow-x: hidden;
  position: relative;
  height: 100%;
  width: 100%;
}

.virtual-spacer {
  position: relative;
  width: 100%;
}

.virtual-content {
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
}

/* 滚动条样式 */
.virtual-tree-container::-webkit-scrollbar {
  width: 8px;
}

.virtual-tree-container::-webkit-scrollbar-track {
  background: transparent;
}

.virtual-tree-container::-webkit-scrollbar-thumb {
  background: var(--border-color);
  border-radius: 4px;
}

.virtual-tree-container::-webkit-scrollbar-thumb:hover {
  background: var(--text-tertiary);
}
</style>
