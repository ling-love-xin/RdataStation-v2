<template>
  <div class="scratchpad-app-wrapper">
    <NConfigProvider :theme="naiveTheme">
      <NMessageProvider>
        <div
          ref="scratchpadPanelRef"
          class="scratchpad-panel"
          :class="{ 'dragover-active': isDragOver }"
          :data-drop-hint="t('scratchpad.dragToImport')"
          @dragover.prevent="handleDragOver"
          @dragleave="handleDragLeave"
          @drop.prevent="handleDrop"
        >
          <div class="explorer-titlebar" @dblclick="handleCollapseAll">
            <h2 class="explorer-titlebar-label">{{ t('scratchpad.title') }}</h2>
            <span class="explorer-titlebar-actions">
              <button
                class="toolbar-btn"
                :disabled="isLoading"
                :title="t('scratchpad.import')"
                @click="handleImportFile"
              >
                <Upload :size="16" />
              </button>
              <button
                class="toolbar-btn"
                :disabled="isLoading"
                :title="t('scratchpad.reference')"
                @click="handleAddReference"
              >
                <FolderSymlink :size="16" />
              </button>
              <button
                class="toolbar-btn"
                :class="{ 'toolbar-btn-active': showExplorerFilter }"
                :title="t('scratchpad.search')"
                @click="toggleExplorerFilter"
              >
                <Search :size="16" />
              </button>
              <NDropdown
                trigger="click"
                placement="bottom-end"
                :options="moreMenuOptions"
                @select="handleMoreMenuSelect"
              >
                <button class="toolbar-btn" :title="t('scratchpad.more')">
                  <MoreVertical :size="16" />
                </button>
              </NDropdown>
            </span>
          </div>

          <div class="scratchpad-explorer-body">
            <div v-if="isLoading" class="loading-state">
              <NSpin size="small" />
            </div>

            <div v-else-if="notInitialized" class="empty-state">
              <div class="empty-icon-wrapper">
                <FolderOpen :size="32" />
              </div>
              <div class="empty-title">{{ t('scratchpad.noProjectTitle') }}</div>
              <div class="empty-hint">{{ t('scratchpad.noProjectHint') }}</div>
            </div>

            <div v-else-if="error" class="error-state">
              <span class="error-text">{{ error }}</span>
              <NButton size="tiny" @click="loadFiles">{{ t('scratchpad.retry') }}</NButton>
            </div>

            <div v-else-if="showExplorerFilter" class="search-panel">
              <div class="search-panel-header">
                <div class="search-input-wrapper">
                  <NInput
                    v-model:value="searchQuery"
                    size="small"
                    :placeholder="t('scratchpad.search')"
                    class="search-panel-input"
                  />
                  <div class="search-toggle-buttons">
                    <button
                      class="toggle-btn"
                      :class="{ 'toggle-btn-active': caseSensitive }"
                      :title="t('scratchpad.matchCase')"
                      @click="caseSensitive = !caseSensitive"
                      >Aa</button
                    >
                    <button
                      class="toggle-btn"
                      :class="{ 'toggle-btn-active': wholeWord }"
                      :title="t('scratchpad.matchWholeWord')"
                      @click="wholeWord = !wholeWord"
                      ><WholeWord :size="13"
                    /></button>
                    <button
                      class="toggle-btn"
                      :class="{ 'toggle-btn-active': isRegex }"
                      :title="t('scratchpad.useRegex')"
                      @click="toggleRegex"
                      >.*</button
                    >
                  </div>
                </div>
                <button
                  class="toolbar-btn"
                  :title="t('scratchpad.closeSearch')"
                  @click="toggleExplorerFilter"
                  ><X :size="14"
                /></button>
              </div>

              <div v-if="regexError" class="search-notice search-notice-error">
                <NIcon size="14"><Info /></NIcon>
                <span>{{ regexError }}</span>
              </div>

              <div class="search-filter-toggle" @click="showSearchFilters = !showSearchFilters">
                <NIcon size="12"
                  ><component :is="showSearchFilters ? ChevronDown : ChevronRight"
                /></NIcon>
                <span>{{ t('scratchpad.filesToInclude') }}</span>
              </div>
              <div v-show="showSearchFilters" class="search-filters">
                <NInput
                  v-model:value="searchInclude"
                  size="small"
                  :placeholder="t('scratchpad.includePattern')"
                  class="search-filter-input"
                />
                <NInput
                  v-model:value="searchExclude"
                  size="small"
                  :placeholder="t('scratchpad.excludePattern')"
                  class="search-filter-input"
                />
              </div>

              <div v-if="showReplaceBar" class="search-replace-bar">
                <NInput
                  v-model:value="replaceWith"
                  size="small"
                  :placeholder="t('scratchpad.replaceWith')"
                  class="search-replace-input"
                />
                <NButton size="small" :loading="replaceInProgress" @click="handleReplaceAll">{{
                  t('scratchpad.replaceAll')
                }}</NButton>
              </div>

              <div v-if="searchQuery.trim()" class="search-results">
                <div v-if="searchResult && searchResult.matches.length > 0">
                  <div
                    v-for="[file, matches] in searchResultsByFile"
                    :key="file"
                    class="search-result-group"
                  >
                    <div class="search-result-file-header" @click="toggleSearchFileExpand(file)">
                      <NIcon size="12"
                        ><component
                          :is="expandedSearchFiles.has(file) ? ChevronDown : ChevronRight"
                      /></NIcon>
                      <NIcon size="14"><FileText /></NIcon>
                      <span class="search-result-file-name">{{ file }}</span>
                      <span class="search-result-match-count">{{ matches.length }}</span>
                    </div>
                    <div v-show="expandedSearchFiles.has(file)" class="search-result-matches">
                      <div
                        v-for="match in matches"
                        :key="`${match.file}:${match.line_number}`"
                        class="search-result-line"
                        @click="handleSearchMatchClick(match)"
                      >
                        <span class="search-result-line-number">{{ match.line_number }}</span>
                        <!-- eslint-disable vue/no-v-html -->
                        <span
                          class="search-result-line-content"
                          v-html="highlightSearchMatch(match)"
                        ></span>
                        <!-- eslint-enable vue/no-v-html -->
                      </div>
                    </div>
                  </div>
                </div>
                <div v-else-if="searchQuery.trim()" class="search-no-results">
                  <span>{{ t('scratchpad.noResults') }}</span>
                </div>
                <div v-if="resultsSummaryText" class="search-results-footer">
                  {{ resultsSummaryText }}
                </div>
              </div>
              <div v-else class="search-no-results">
                <span>{{ t('scratchpad.typeToSearch') }}</span>
              </div>
            </div>

            <div
              v-else
              ref="treeContainerRef"
              class="tree-container"
              @contextmenu.prevent="showBlankMenu($event)"
            >
              <div v-if="recentFileEntries.length > 0" class="pane-section">
                <div
                  class="pane-section-header"
                  @click="showRecent = !showRecent"
                  @contextmenu.stop
                >
                  <NIcon size="14">
                    <component :is="showRecent ? ChevronDown : ChevronRight" />
                  </NIcon>
                  <NIcon size="14"><History /></NIcon>
                  <h3 class="pane-section-title">{{ t('scratchpad.recentFiles') }}</h3>
                  <span class="pane-section-actions">
                    <button
                      class="toolbar-btn"
                      :title="t('scratchpad.newFile')"
                      @click.stop="handleCreateFile"
                      ><FilePlus :size="14"
                    /></button>
                    <button
                      class="toolbar-btn"
                      :title="t('scratchpad.saveAll')"
                      @click.stop="handleSaveAll"
                      ><FileText :size="14"
                    /></button>
                    <button
                      class="toolbar-btn"
                      :title="t('scratchpad.closeRecent')"
                      @click.stop="handleCloseRecent"
                      ><X :size="14"
                    /></button>
                  </span>
                </div>
                <div v-show="showRecent" class="pane-section-body">
                  <div
                    v-for="entry in recentFileEntries"
                    :key="entry.path"
                    class="recent-entry"
                    @click="handleOpen(entry)"
                  >
                    <NIcon size="14"><FileText /></NIcon>
                    <span class="recent-name">{{ entry.name }}</span>
                  </div>
                </div>
              </div>

              <div class="pane-section pane-section-main" :style="getSectionFlexStyle('local')">
                <div class="pane-section-header" @click="toggleGroup('local')" @contextmenu.stop>
                  <NIcon size="14">
                    <component :is="groupExpanded.local ? ChevronDown : ChevronRight" />
                  </NIcon>
                  <NIcon size="14"><Folder /></NIcon>
                  <h3 class="pane-section-title">{{ t('scratchpad.localDrafts') }}</h3>
                  <span class="pane-section-actions">
                    <button
                      class="toolbar-btn"
                      :title="t('scratchpad.newFile')"
                      @click.stop="handleCreateFile"
                      ><FilePlus :size="14"
                    /></button>
                    <button
                      class="toolbar-btn"
                      :title="t('scratchpad.newFolder')"
                      @click.stop="handleCreateFolder"
                      ><FolderPlus :size="14"
                    /></button>
                    <button
                      class="toolbar-btn"
                      :title="t('scratchpad.import')"
                      @click.stop="handleImportFile"
                      ><Upload :size="14"
                    /></button>
                    <button
                      class="toolbar-btn"
                      :title="t('scratchpad.collapseAll')"
                      @click.stop="handleCollapseAll"
                      ><ChevronsUpDown :size="14"
                    /></button>
                  </span>
                </div>
                <div v-show="groupExpanded.local" class="pane-section-body">
                  <div
                    v-if="isInlineCreateActive && inlineCreateParentPath === null"
                    class="inline-tree-entry"
                  >
                    <NIcon size="14" class="node-icon">
                      <component :is="inlineCreateIsFolder ? Folder : FileText" />
                    </NIcon>
                    <input
                      ref="rootInlineInputRef"
                      v-model="rootInlineCreateName"
                      class="rename-input"
                      :placeholder="
                        inlineCreateIsFolder ? t('scratchpad.newFolder') : t('scratchpad.newFile')
                      "
                      @keyup.enter="commitRootInlineCreate"
                      @keyup.escape="cancelRootInlineCreate"
                      @click.stop
                    />
                  </div>
                  <div
                    v-if="filteredLocalEntries.length === 0 && inlineCreateParentPath !== null"
                    class="empty-state"
                  >
                    <div class="empty-icon-wrapper">
                      <NIcon size="32"><FolderOpen /></NIcon>
                    </div>
                    <span class="empty-title">{{ t('scratchpad.emptyScratchpad') }}</span>
                    <span class="empty-hint">{{ t('scratchpad.emptyScratchpadHint') }}</span>
                    <div class="empty-actions">
                      <NButton size="small" type="primary" @click="handleCreateFile">
                        <template #icon>
                          <NIcon size="14"><FilePlus /></NIcon>
                        </template>
                        {{ t('scratchpad.newFile') }}
                      </NButton>
                      <NButton size="small" @click="handleImportFile">
                        <template #icon>
                          <NIcon size="14"><Upload /></NIcon>
                        </template>
                        {{ t('scratchpad.import') }}
                      </NButton>
                    </div>
                  </div>
                  <div
                    v-if="useVirtualScrollEnabled"
                    class="virtual-scroll-viewport"
                    :style="{ height: virtualScrollTotalHeight + 'px' }"
                  >
                    <div
                      class="virtual-scroll-spacer"
                      :style="{ height: virtualScrollPaddingTop + 'px' }"
                    />
                    <ScratchpadTreeNode
                      v-for="item in visibleTreeEntries"
                      :key="item.entry.path"
                      :entry="item.entry"
                      :depth="item.depth"
                      :expanded-keys="expandedKeys"
                      :selected-key="selectedKey"
                      :selected-keys="selectedKeys"
                      :renaming-key="renamingKey"
                      :inline-create-parent-path="inlineCreateParentPath"
                      :inline-create-is-folder="inlineCreateIsFolder"
                      :dirty-files="dirtyFiles"
                      @select="handleSelect"
                      @open="handleOpen"
                      @contextmenu="showEntryMenu"
                      @toggle-expand="handleToggleExpand"
                      @start-rename="startRename"
                      @finish-rename="finishRename"
                      @cancel-rename="cancelRename"
                      @drag-start="handleTreeNodeDragStart"
                      @create-inline="confirmInlineCreate"
                      @drop-file="handleNodeDrop"
                    />
                  </div>
                  <template v-else>
                    <ScratchpadTreeNode
                      v-for="item in flattenedTree"
                      :key="item.entry.path"
                      :entry="item.entry"
                      :depth="item.depth"
                      :expanded-keys="expandedKeys"
                      :selected-key="selectedKey"
                      :selected-keys="selectedKeys"
                      :renaming-key="renamingKey"
                      :inline-create-parent-path="inlineCreateParentPath"
                      :inline-create-is-folder="inlineCreateIsFolder"
                      :dirty-files="dirtyFiles"
                      @select="handleSelect"
                      @open="handleOpen"
                      @contextmenu="showEntryMenu"
                      @toggle-expand="handleToggleExpand"
                      @start-rename="startRename"
                      @finish-rename="finishRename"
                      @cancel-rename="cancelRename"
                      @drag-start="handleTreeNodeDragStart"
                      @create-inline="confirmInlineCreate"
                      @drop-file="handleNodeDrop"
                    />
                  </template>
                </div>
              </div>

              <div
                v-if="groupExpanded.local && groupExpanded.external"
                class="section-resize-handle"
                :class="{ 'section-resize-active': resizing === 'local' }"
                @mousedown="startResize('local', $event)"
              ></div>

              <div class="pane-section" :style="getSectionFlexStyle('external')">
                <div class="pane-section-header" @click="toggleGroup('external')" @contextmenu.stop>
                  <NIcon size="14">
                    <component :is="groupExpanded.external ? ChevronDown : ChevronRight" />
                  </NIcon>
                  <NIcon size="14"><FolderSymlink /></NIcon>
                  <h3 class="pane-section-title">{{ t('scratchpad.externalReferences') }}</h3>
                  <span class="pane-section-actions">
                    <button
                      class="toolbar-btn"
                      :title="t('scratchpad.reference')"
                      @click.stop="handleAddReference"
                      ><FolderSymlink :size="14"
                    /></button>
                    <button
                      class="toolbar-btn"
                      :title="t('scratchpad.collapseAll')"
                      @click.stop="toggleGroup('external')"
                      ><ChevronsUpDown :size="14"
                    /></button>
                  </span>
                </div>
                <div v-show="groupExpanded.external" class="pane-section-body">
                  <div v-if="externalReferences.length === 0" class="empty-hint">
                    {{ t('scratchpad.noReferences') }}
                  </div>
                  <div
                    v-for="extRef in externalReferences"
                    :key="extRef.alias"
                    class="ref-row"
                    @click="handleRefClick(extRef)"
                    @contextmenu.prevent="showRefMenu($event, extRef)"
                  >
                    <NIcon size="14">
                      <FolderSymlink :class="{ 'ref-invalid': isRefInvalid(extRef) }" />
                    </NIcon>
                    <span class="ref-name">{{ extRef.alias }}</span>
                    <span
                      :class="['ref-path', { 'ref-path-invalid': isRefInvalid(extRef) }]"
                      :title="extRef.path"
                      >{{ extRef.path }}</span
                    >
                    <span v-if="isRefInvalid(extRef)" class="ref-badge">{{
                      t('scratchpad.invalid')
                    }}</span>
                  </div>
                </div>
              </div>

              <div
                v-if="groupExpanded.external && showTrash"
                class="section-resize-handle"
                :class="{ 'section-resize-active': resizing === 'external' }"
                @mousedown="startResize('external', $event)"
              ></div>

              <div class="pane-section" :style="getSectionFlexStyle('trash')">
                <div class="pane-section-header" @click="toggleTrash" @contextmenu.stop>
                  <NIcon size="14">
                    <component :is="showTrash ? ChevronDown : ChevronRight" />
                  </NIcon>
                  <NIcon size="14"><Trash2 /></NIcon>
                  <h3 class="pane-section-title">{{ t('scratchpad.trash') }}</h3>
                  <span class="pane-section-actions">
                    <button
                      v-if="trashEntries.length > 0"
                      class="toolbar-btn"
                      :title="t('scratchpad.emptyTrash')"
                      @click.stop="handleEmptyTrash"
                      ><X :size="14"
                    /></button>
                    <button
                      class="toolbar-btn"
                      :title="t('scratchpad.collapseAll')"
                      @click.stop="toggleTrash"
                      ><ChevronsUpDown :size="14"
                    /></button>
                  </span>
                </div>
                <div v-show="showTrash" class="pane-section-body">
                  <div v-if="trashEntries.length === 0" class="empty-hint">
                    {{ t('scratchpad.noTrashItems') }}
                  </div>
                  <div v-for="item in trashEntries" :key="item.path" class="ref-row">
                    <NIcon size="14"><FileText /></NIcon>
                    <span class="ref-name">{{ item.name }}</span>
                    <span class="ref-path" :title="item.path">{{ item.path }}</span>
                    <NButton
                      size="tiny"
                      quaternary
                      type="primary"
                      @click.stop="handleRestore(item.name)"
                    >
                      {{ t('scratchpad.restoreFromTrash') }}
                    </NButton>
                  </div>
                </div>
              </div>
            </div>
          </div>

          <NModal v-model:show="showRefModal" :mask-closable="false" style="max-width: 520px">
            <NConfigProvider :theme="naiveTheme">
              <NCard
                :title="t('scratchpad.addReference')"
                size="small"
                :content-style="{
                  display: 'flex',
                  flexDirection: 'column',
                  gap: 'var(--spacing-md)',
                }"
              >
                <div class="modal-field">
                  <label class="modal-label">{{ t('scratchpad.aliasLabel') }}</label>
                  <NInput
                    v-model:value="newRefAlias"
                    :placeholder="t('scratchpad.aliasPlaceholder')"
                  />
                </div>
                <div class="modal-field">
                  <label class="modal-label">{{ t('scratchpad.pathLabel') }}</label>
                  <div class="modal-input-row">
                    <NInput
                      v-model:value="newRefPath"
                      :placeholder="t('scratchpad.pathPlaceholder')"
                      class="modal-input"
                    />
                    <NButton size="small" @click="browseRefPath">
                      <template #icon>
                        <NIcon><FolderOpen /></NIcon>
                      </template>
                      {{ t('scratchpad.browse') }}
                    </NButton>
                  </div>
                </div>
                <template #footer>
                  <div class="modal-actions">
                    <NButton size="small" @click="showRefModal = false">{{
                      t('common.cancel')
                    }}</NButton>
                    <NButton
                      size="small"
                      type="primary"
                      :disabled="!newRefAlias.trim() || !newRefPath.trim()"
                      @click="confirmAddReference"
                      >{{ t('common.confirm') }}</NButton
                    >
                  </div>
                </template>
              </NCard>
            </NConfigProvider>
          </NModal>

          <NModal v-model:show="showPromoteConfirm" :mask-closable="false" style="max-width: 460px">
            <NConfigProvider :theme="naiveTheme">
              <NCard size="small">
                <template #header>
                  <div class="modal-header-row">
                    <NIcon size="20" color="var(--brand-accent)"><Info /></NIcon>
                    <span class="modal-header-title">{{ t('scratchpad.promoteToResource') }}</span>
                  </div>
                </template>
                <template v-if="promoteTarget">
                  {{ t('scratchpad.promoteToResourceConfirm', { name: promoteTarget.name }) }}
                </template>
                <template #footer>
                  <div class="modal-actions">
                    <NButton size="small" @click="showPromoteConfirm = false">{{
                      t('common.cancel')
                    }}</NButton>
                    <NButton size="small" @click="handlePromoteConfirm(true)">{{
                      t('scratchpad.promoteDeleteDraft')
                    }}</NButton>
                    <NButton size="small" type="primary" @click="handlePromoteConfirm(false)">{{
                      t('scratchpad.promoteKeepDraft')
                    }}</NButton>
                  </div>
                </template>
              </NCard>
            </NConfigProvider>
          </NModal>

          <NModal v-model:show="showConflictDialog" :mask-closable="false" style="max-width: 480px">
            <NConfigProvider :theme="naiveTheme">
              <NCard size="small">
                <template #header>
                  <div class="modal-header-row">
                    <NIcon size="20" color="var(--brand-warning)"><Info /></NIcon>
                    <span class="modal-header-title">{{ t('scratchpad.conflictDetected') }}</span>
                  </div>
                </template>
                <template v-if="conflictFilePath">
                  <div class="conflict-message">
                    <p>{{ t('scratchpad.conflictMessage1', { path: conflictFilePath }) }}</p>
                    <p>{{ t('scratchpad.conflictMessage2') }}</p>
                  </div>
                  <div class="conflict-actions">
                    <NButton size="small" type="info" @click="handleConflictDiff">
                      {{ t('scratchpad.viewDiff') }}
                    </NButton>
                  </div>
                </template>
                <template #footer>
                  <div class="modal-actions">
                    <NButton size="small" @click="handleConflictIgnore">{{
                      t('scratchpad.ignoreAction')
                    }}</NButton>
                    <NButton size="small" type="primary" @click="handleConflictReload">{{
                      t('scratchpad.reloadAction')
                    }}</NButton>
                  </div>
                </template>
              </NCard>
            </NConfigProvider>
          </NModal>

          <NModal
            v-model:show="showDiffDialog"
            :title="t('scratchpad.diffView')"
            preset="card"
            class="diff-modal"
            :mask-closable="false"
          >
            <template #header-extra>
              <NButton size="small" @click="handleDiffClose">{{ t('common.back') }}</NButton>
            </template>
            <div v-if="diffResult" class="diff-container">
              <div class="diff-labels">
                <span class="diff-label-left">{{ diffLeftLabel }}</span>
                <span class="diff-label-right">{{ diffRightLabel }}</span>
              </div>
              <div class="diff-lines">
                <div
                  v-for="(line, idx) in diffResult.lines"
                  :key="idx"
                  :class="['diff-line', `diff-${line.kind}`]"
                >
                  <span class="diff-line-num diff-num-left">{{ line.line_number_left || '' }}</span>
                  <span class="diff-line-num diff-num-right">{{
                    line.line_number_right || ''
                  }}</span>
                  <span class="diff-line-content">{{ line.content }}</span>
                </div>
              </div>
            </div>
            <template #footer>
              <NButton size="small" @click="handleDiffClose">{{ t('common.close') }}</NButton>
              <NButton size="small" type="info" @click="handleDiffAcceptRight">
                {{ t('scratchpad.acceptRight') }}
              </NButton>
            </template>
          </NModal>

          <div v-if="moveUndoVisible" class="undo-bar move-undo-bar">
            <span class="undo-text">{{
              t('scratchpad.undoMoveHint', { name: moveUndoName })
            }}</span>
            <NButton size="tiny" type="primary" quaternary @click="handleMoveUndo">
              {{ t('scratchpad.undo') }}
            </NButton>
            <NButton size="tiny" quaternary @click="dismissMoveUndo">
              <template #icon>
                <NIcon size="12"><X /></NIcon>
              </template>
            </NButton>
          </div>

          <Teleport to="body">
            <Transition name="context-menu-fade">
              <div
                v-if="contextMenu.visible"
                class="scratchpad-context-menu"
                :style="{ left: `${contextMenu.x}px`, top: `${contextMenu.y}px` }"
              >
                <template v-for="item in contextMenu.items" :key="item.key">
                  <div v-if="item.type === 'divider'" class="menu-divider" />
                  <div
                    v-else
                    :class="[
                      'menu-item',
                      { 'menu-item-danger': item.danger, 'menu-item-disabled': item.disabled },
                    ]"
                    @click="!item.disabled && handleMenuAction(item.key)"
                  >
                    <NIcon v-if="item.icon" size="14"><component :is="item.icon" /></NIcon>
                    <span>{{ item.label }}</span>
                    <span v-if="item.shortcut" class="menu-shortcut">{{ item.shortcut }}</span>
                  </div>
                </template>
              </div>
            </Transition>
          </Teleport>

          <div v-if="undoState.visible" class="undo-bar">
            <span class="undo-text">{{
              t('scratchpad.undoDeleteHint', { name: undoState.name })
            }}</span>
            <NButton size="tiny" type="primary" quaternary @click="handleUndoDelete">
              {{ t('scratchpad.undo') }}
            </NButton>
            <NButton size="tiny" quaternary @click="dismissUndo">
              <template #icon>
                <NIcon size="12"><X /></NIcon>
              </template>
            </NButton>
          </div>
        </div>
      </NMessageProvider>
    </NConfigProvider>
  </div>
</template>

<script setup lang="ts">
import { listen } from '@tauri-apps/api/event'
import {
  FilePlus,
  FolderPlus,
  Folder,
  FolderOpen,
  Upload,
  Search,
  ChevronDown,
  ChevronRight,
  ChevronsUpDown,
  FolderSymlink,
  FileText,
  Trash2,
  X,
  Info,
  History,
  MoreVertical,
  WholeWord,
} from 'lucide-vue-next'
import {
  NButton,
  NIcon,
  NInput,
  NSpin,
  NModal,
  NCard,
  NDropdown,
  createDiscreteApi,
  NConfigProvider,
  NMessageProvider,
  darkTheme,
  lightTheme,
} from 'naive-ui'
import { ref, reactive, computed, watch, onMounted, onUnmounted, nextTick } from 'vue'
import { useI18n } from 'vue-i18n'

import { useAppStore } from '@/stores/useAppStore'
import { useScratchpadEditorStore } from '@/stores/useScratchpadEditorStore'

import ScratchpadTreeNode from './ScratchpadTreeNode.vue'
import { useScratchpad } from '../composables/use-scratchpad'
import { useContextMenu } from '../composables/useContextMenu'
import { useFileActions } from '../composables/useFileActions'
import { useKeyboard } from '../composables/useKeyboard'

import type {
  ScratchpadChangeEvent,
  ScratchpadEntry,
  ExternalReference,
  SearchMatch,
  SearchResult,
  DiffResult,
} from '../../types'
import type { ScratchpadApi } from '../composables/useFileActions'

const { t } = useI18n()
const appStore = useAppStore()
const naiveTheme = computed(() => (appStore.isDark ? darkTheme : lightTheme))
const { message } = createDiscreteApi(['message'])

const {
  response,
  isLoading,
  notInitialized,
  error,
  searchQuery,
  localEntries,
  externalReferences,
  scratchpadPath,
  loadFiles,
  createEntry,
  deleteEntry,
  renameEntry,
  loadFileContent,
  saveFile,
  importFile,
  addReference,
  removeReference,
  isRefInvalid,
  openInExplorerAction,
  searchContent,
  trashEntries,
  loadTrashEntries,
  restoreTrashEntry,
  emptyTrashBin,
  analyzableFiles,
  loadAnalyzableFiles,
  startWatching,
  stopWatching,
  promoteToResource,
  applyFileChanges,
  flattenVisibleEntries,
  dirtyFiles,
  markClean,
  externalConflicts,
  dismissConflict,
  loadChildEntries,
  hasChildrenLoaded,
  clipboardMode,
  moveEntry,
  replaceInFile,
  diffWithContent,
  validateRegex,
  setContentSnapshot,
  getContentSnapshot,
  normalizePathForCompare,
} = useScratchpad()

const fileMeta = computed(() => response.value?.file_meta ?? {})

const _editorStore = useScratchpadEditorStore()

const contentSearchMode = ref(false)
const searchResult = ref<SearchResult | null>(null)
const showPromoteConfirm = ref(false)
const promoteTarget = ref<ScratchpadEntry | null>(null)
const showConflictDialog = ref(false)
const conflictFilePath = ref<string | null>(null)
const showTrash = ref(false)
const sortBy = ref<'name' | 'size' | 'modified'>('name')
const sortOrder = ref<'asc' | 'desc'>('asc')
const showExplorerFilter = ref(false)
const caseSensitive = ref(false)
const isRegex = ref(false)
const regexError = ref<string | null>(null)
const wholeWord = ref(false)
const expandedSearchFiles = ref<Set<string>>(new Set())
const showSearchFilters = ref(false)
const searchInclude = ref('')
const searchExclude = ref('')
const showRecent = ref(true)
const recentFiles = ref<string[]>([])
const sectionFlex = reactive<Record<string, number>>({ local: 60, external: 20, trash: 20 })
const resizing = ref<string | null>(null)
const resizeStartY = ref(0)
const resizeStartFlex = ref<[number, number]>([0, 0])
const MAX_RECENT = 5
let contentSearchTimer: ReturnType<typeof setTimeout> | null = null

const showReplaceBar = ref(false)
const replaceWith = ref('')
const replaceInProgress = ref(false)

const showDiffDialog = ref(false)
const diffResult = ref<DiffResult | null>(null)
const diffLeftLabel = ref('')
const diffRightLabel = ref('')
const diffFilePath = ref('')

const moveUndoVisible = ref(false)
const moveUndoFromPath = ref('')
const moveUndoToParent = ref('')
const moveUndoName = ref('')
let moveUndoTimer: ReturnType<typeof setTimeout> | null = null
const MOVE_UNDO_TIMEOUT = 5000

function toggleExplorerFilter(): void {
  showExplorerFilter.value = !showExplorerFilter.value
  if (showExplorerFilter.value) {
    contentSearchMode.value = true
    searchQuery.value = ''
    searchResult.value = null
    showReplaceBar.value = false
    expandedSearchFiles.value = new Set()
    nextTick(() => {
      const input = document.querySelector('.search-panel-input input') as HTMLInputElement | null
      input?.focus()
    })
  } else {
    searchQuery.value = ''
    contentSearchMode.value = false
    searchResult.value = null
    showReplaceBar.value = false
  }
}

const moreMenuOptions = computed(() => [
  { label: t('scratchpad.newFile'), key: 'newFile' },
  { label: t('scratchpad.newFolder'), key: 'newFolder' },
  { label: t('scratchpad.refresh'), key: 'refresh' },
  { type: 'divider' as const, key: 'd1' },
  { label: t('scratchpad.sortByName'), key: 'sortName' },
  { label: t('scratchpad.sortBySize'), key: 'sortSize' },
  { label: t('scratchpad.sortByModified'), key: 'sortModified' },
  { type: 'divider' as const, key: 'd2' },
  { label: t('scratchpad.expandAll'), key: 'expandAll' },
  { label: t('scratchpad.collapseAll'), key: 'collapseAll' },
])

function handleMoreMenuSelect(key: string): void {
  switch (key) {
    case 'newFile':
      handleCreateFile()
      break
    case 'newFolder':
      handleCreateFolder()
      break
    case 'refresh':
      loadFiles()
      break
    case 'sortName':
      toggleSort('name')
      break
    case 'sortSize':
      toggleSort('size')
      break
    case 'sortModified':
      toggleSort('modified')
      break
    case 'expandAll':
      handleExpandAll()
      break
    case 'collapseAll':
      handleCollapseAll()
      break
  }
}

const searchResultsByFile = computed(() => {
  const sr = searchResult.value
  if (!sr || sr.matches.length === 0) return new Map<string, SearchMatch[]>()
  const grouped = new Map<string, SearchMatch[]>()
  for (const m of sr.matches) {
    const list = grouped.get(m.file) || []
    list.push(m)
    grouped.set(m.file, list)
  }
  return grouped
})

const resultsSummaryText = computed(() => {
  const sr = searchResult.value
  if (!sr) return ''
  const fileCount = searchResultsByFile.value.size
  const matchCount = sr.matches.length
  if (matchCount === 0) return t('scratchpad.noResults')
  return t('scratchpad.searchSummary', { files: fileCount, count: matchCount })
})

function toggleSearchFileExpand(file: string): void {
  const next = new Set(expandedSearchFiles.value)
  if (next.has(file)) {
    next.delete(file)
  } else {
    next.add(file)
  }
  expandedSearchFiles.value = next
}

function handleSearchMatchClick(match: SearchMatch): void {
  openFileAtLine(match.file, match.line_number)
}

function highlightSearchMatch(match: SearchMatch): string {
  const line = escapeHtml(match.line_content)
  const q = searchQuery.value.trim()
  if (!q || isRegex.value) return line
  const lowerLine = caseSensitive.value ? line : line.toLowerCase()
  const lowerQ = caseSensitive.value ? q : q.toLowerCase()
  const idx = lowerLine.indexOf(lowerQ)
  if (idx === -1) return line
  const before = line.slice(0, idx)
  const matched = line.slice(idx, idx + q.length)
  const after = line.slice(idx + q.length)
  return `${escapeHtml(before)}<mark class="search-hl">${escapeHtml(matched)}</mark>${escapeHtml(after)}`
}

function toggleRegex(): void {
  isRegex.value = !isRegex.value
  regexError.value = null
  if (isRegex.value) {
    const result = validateRegex(searchQuery.value)
    regexError.value = result.error
  }
}

function toggleSort(field: 'name' | 'size' | 'modified'): void {
  if (sortBy.value === field) {
    sortOrder.value = sortOrder.value === 'asc' ? 'desc' : 'asc'
  } else {
    sortBy.value = field
    sortOrder.value = 'asc'
  }
}

const contentResults = computed(() => {
  const sr = searchResult.value
  if (!sr) return new Map<string, SearchMatch[]>()
  const grouped = new Map<string, SearchMatch[]>()
  for (const m of sr.matches) {
    const list = grouped.get(m.file) || []
    list.push(m)
    grouped.set(m.file, list)
  }
  return grouped
})

watch(searchQuery, async val => {
  if (!contentSearchMode.value) return
  if (contentSearchTimer) clearTimeout(contentSearchTimer)
  if (!val.trim()) {
    searchResult.value = null
    return
  }
  contentSearchTimer = setTimeout(async () => {
    const result = await searchContent(val.trim(), caseSensitive.value)
    searchResult.value = result
  }, 300)
})

watch(searchResult, sr => {
  if (sr && sr.matches.length > 0) {
    const files = new Set(sr.matches.map(m => m.file))
    expandedSearchFiles.value = new Set(files)
  }
})

watch(caseSensitive, async () => {
  const q = searchQuery.value.trim()
  if (!contentSearchMode.value || !q) return
  const result = await searchContent(q, caseSensitive.value)
  searchResult.value = result
})

const recentFileEntries = computed(() => {
  if (recentFiles.value.length === 0) return []
  const scratchpadBase = scratchpadPath.value || ''
  return recentFiles.value
    .map(rp =>
      localEntries.value.find(e => {
        const relPath = scratchpadBase
          ? e.path.replace(scratchpadBase.replace(/\\/g, '/'), '').replace(/^\//, '')
          : e.path
        return relPath === rp
      })
    )
    .filter((e): e is ScratchpadEntry => !!e)
})

function addRecentFile(relativePath: string): void {
  const list = recentFiles.value.filter(p => p !== relativePath)
  list.unshift(relativePath)
  recentFiles.value = list.slice(0, MAX_RECENT)
}

watch(searchQuery, async val => {
  if (isRegex.value) {
    const result = validateRegex(val || '')
    regexError.value = result.error
  }
})

async function handleReplaceAll(): Promise<void> {
  const q = searchQuery.value.trim()
  const rw = replaceWith.value
  if (!q || !rw) return
  replaceInProgress.value = true
  const files = contentResults.value
  let totalFiles = 0
  let totalReplaced = 0
  for (const [file] of files) {
    const result = await replaceInFile(file, q, rw, isRegex.value)
    if (result && result.replaced > 0) {
      totalFiles++
      totalReplaced += result.replaced
    }
  }
  replaceInProgress.value = false
  if (totalReplaced > 0) {
    message.success(t('scratchpad.replaceSuccess', { files: totalFiles, count: totalReplaced }))
    const result = await searchContent(q, caseSensitive.value)
    searchResult.value = result
  } else {
    message.info(t('scratchpad.replaceNoMatches'))
  }
}

const filteredLocalEntries = computed(() => {
  let entries = localEntries.value
  if (contentSearchMode.value) {
    const results = contentResults.value
    const base = scratchpadPath.value || ''
    entries = entries.filter(e => {
      let rel = e.path
      if (base) {
        rel = e.path.replace(base.replace(/\\/g, '/'), '').replace(/^\//, '')
      }
      return results.has(rel)
    })
  }
  return [...entries].sort((a, b) => {
    /* folders first */
    if (a.kind !== b.kind) {
      return a.kind === 'folder' ? -1 : 1
    }
    let cmp = 0
    if (sortBy.value === 'name') {
      cmp = a.name.localeCompare(b.name)
    } else if (sortBy.value === 'size') {
      cmp = a.size - b.size
    } else if (sortBy.value === 'modified') {
      const da = a.modified_at || ''
      const db = b.modified_at || ''
      cmp = da.localeCompare(db)
    }
    return sortOrder.value === 'asc' ? cmp : -cmp
  })
})

const multiSelected = computed(() => selectedKeys.value.size)

const expandedKeys = ref<Set<string>>(new Set())
const selectedKey = ref<string | null>(null)
const selectedKeys = ref<Set<string>>(new Set())
const lastSelectPath = ref<string | null>(null)
const clipboardEntry = ref<ScratchpadEntry | null>(null)
const renamingKey = ref<string | null>(null)

const undoState = reactive<{
  visible: boolean
  name: string
  relativePath: string
  timer: ReturnType<typeof setTimeout> | null
}>({
  visible: false,
  name: '',
  relativePath: '',
  timer: null,
})

const UNDO_TIMEOUT = 5000

const ROW_HEIGHT = 28
const OVERSCAN = 8
const VIRTUAL_SCROLL_THRESHOLD = 50

const treeContainerRef = ref<HTMLElement | null>(null)
const scratchpadPanelRef = ref<HTMLElement | null>(null)
const treeScrollTop = ref(0)
const treeContainerHeight = ref(0)

const flattenedTree = computed(() => {
  const tree = flattenVisibleEntries(filteredLocalEntries.value, expandedKeys.value)
  const seen = new Map<string, number>()
  for (const item of tree) {
    const key = normalizePathForCompare(item.entry.path)
    seen.set(key, (seen.get(key) || 0) + 1)
  }
  for (const [key, count] of seen) {
    if (count > 1) {
      console.warn(
        '[flattenedTree] DUPLICATE across levels! path:',
        key,
        'count:',
        count,
        'entries:',
        tree
          .filter(i => normalizePathForCompare(i.entry.path) === key)
          .map(i => ({ name: i.entry.name, kind: i.entry.kind, depth: i.depth }))
      )
    }
  }
  const testDir = tree.filter(i => i.entry.path.includes('测试2026'))
  if (testDir.length > 0) {
    // eslint-disable-next-line no-console
    console.log(
      '[flattenedTree] 测试2026 entries:',
      testDir.map(i => `${i.depth}:${i.entry.kind}:${i.entry.name}`)
    )
    const children = testDir.filter(i => i.entry.kind === 'file')
    if (children.length > 0) {
      // eslint-disable-next-line no-console
      console.log(
        '[flattenedTree] 测试2026 files:',
        children.map(i => `${i.entry.name}@${i.entry.path}@depth${i.depth}`)
      )
    }
  }
  return tree
})

const useVirtualScrollEnabled = computed(
  () => flattenedTree.value.length > VIRTUAL_SCROLL_THRESHOLD
)

const visibleTreeEntries = computed(() => {
  if (!useVirtualScrollEnabled.value) {
    return flattenedTree.value
  }
  const total = flattenedTree.value.length
  const start = Math.floor(treeScrollTop.value / ROW_HEIGHT)
  const visibleCount = Math.ceil(treeContainerHeight.value / ROW_HEIGHT)
  const from = Math.max(0, start - OVERSCAN)
  const to = Math.min(total, start + visibleCount + OVERSCAN)
  return flattenedTree.value.slice(from, to)
})

const virtualScrollPaddingTop = computed(() => {
  if (!useVirtualScrollEnabled.value || flattenedTree.value.length === 0) return 0
  const start = Math.floor(treeScrollTop.value / ROW_HEIGHT)
  const from = Math.max(0, start - OVERSCAN)
  return from * ROW_HEIGHT
})

const virtualScrollTotalHeight = computed(() => {
  if (!useVirtualScrollEnabled.value) return undefined
  return flattenedTree.value.length * ROW_HEIGHT
})

const contextMenuComposable = useContextMenu(
  {
    t,
    selectedKeys,
    expandedKeys,
    clipboardEntry,
    multiSelected,
  },
  {
    onNewFile: parentPath => startInlineCreate(parentPath, false),
    onNewFolder: parentPath => startInlineCreate(parentPath, true),
    onOpen: entry => openFileInEditor(entry),
    onAnalyzeDuckDB: entry => {
      handleAnalyzeDuckDB(entry)
    },
    onToggleFolder: entry => {
      handleToggleExpand(entry)
    },
    onRename: entry => startRename(entry),
    onDelete: entry => {
      deleteEntry(entry.path)
      showUndo(entry.name, entry.name)
    },
    onBatchDelete: paths => {
      const confirmed = window.confirm(t('scratchpad.batchDeleteConfirm', { n: paths.length }))
      if (!confirmed) return
      Promise.all(paths.map(p => deleteEntry(p)))
      message.success(t('scratchpad.batchDeletedSuccess', { n: paths.length }))
      selectedKeys.value = new Set<string>()
    },
    onCopyFile: entry => {
      clipboardEntry.value = entry
      clipboardMode.value = 'copy'
    },
    onCutFile: entry => {
      clipboardEntry.value = entry
      clipboardMode.value = 'cut'
      message.info(t('scratchpad.cutFileHint'))
    },
    onPasteFile: handlePaste,
    onPromote: entry => {
      promoteTarget.value = entry
      showPromoteConfirm.value = true
    },
    onCopyPath: async path => {
      await navigator.clipboard.writeText(path)
      message.success(t('scratchpad.pathCopied'))
    },
    onCopyAbsPath: async path => {
      await navigator.clipboard.writeText(path)
      message.success(t('scratchpad.absolutePathCopied'))
    },
    onRemoveRef: alias => {
      doRemoveReference(alias)
    },
    onOpenRefLocation: async path => {
      await openInExplorerAction(path)
    },
  }
)
const {
  contextMenu,
  showBlankMenu,
  showEntryMenu,
  showRefMenu,
  closeContextMenu,
  handleMenuAction,
} = contextMenuComposable

const keyboardComposable = useKeyboard(
  {
    contextMenuVisible: computed(() => contextMenu.visible),
    showExplorerFilter,
    selectedKey,
    selectedKeys,
    multiSelected,
    flattenedEntries: flattenedTree,
  },
  {
    onCloseContextMenu: () => closeContextMenu(),
    onToggleSearch: () => toggleExplorerFilter(),
    onCreateFile: () => handleCreateFile(),
    onSelectAll: () => {
      const all = new Set<string>()
      for (const item of flattenedTree.value) {
        all.add(item.entry.path)
      }
      selectedKeys.value = all
    },
    onNavigateToEntry: entry => {
      selectedKey.value = entry.path
      scrollToSelected()
    },
    onOpenEntry: entry => handleOpen(entry),
    onRenameEntry: entry => startRename(entry),
    onDeleteEntry: entry => {
      deleteEntry(entry.path)
      showUndo(entry.name, entry.name)
    },
    onBatchDelete: paths => {
      const confirmed = window.confirm(t('scratchpad.batchDeleteConfirm', { n: paths.length }))
      if (!confirmed) return
      Promise.all(paths.map(p => deleteEntry(p)))
      message.success(t('scratchpad.batchDeletedSuccess', { n: paths.length }))
      selectedKeys.value = new Set<string>()
    },
    onEscCloseSearch: () => toggleExplorerFilter(),
  }
)
const { handleKeydown } = keyboardComposable

function updateTreeContainerSize(): void {
  const el = treeContainerRef.value
  if (!el) return
  treeContainerHeight.value = el.clientHeight
}

function showUndo(name: string, relativePath: string): void {
  if (undoState.timer) clearTimeout(undoState.timer)
  undoState.visible = true
  undoState.name = name
  undoState.relativePath = relativePath
  undoState.timer = setTimeout(() => {
    dismissUndo()
  }, UNDO_TIMEOUT)
}

function dismissUndo(): void {
  undoState.visible = false
  if (undoState.timer) {
    clearTimeout(undoState.timer)
    undoState.timer = null
  }
}

async function handleUndoDelete(): Promise<void> {
  const name = undoState.relativePath
  dismissUndo()
  await doUndoDelete(name)
}

const groupExpanded = reactive({ external: false, local: true })

const showRefModal = ref(false)
const newRefAlias = ref('')
const newRefPath = ref('')

const scratchpadApi: ScratchpadApi = {
  createEntry,
  deleteEntry,
  renameEntry,
  importFile,
  promoteToResource,
  emptyTrashBin,
  restoreTrashEntry,
  addReference,
  removeReference,
  loadFileContent,
  setContentSnapshot,
  error,
}

const fileActionsComposable = useFileActions({
  api: scratchpadApi,
  store: _editorStore,
  message: {
    success: (msg: string) => message.success(msg),
    error: (msg: string) => message.error(msg),
    info: (msg: string) => message.info(msg),
    warning: (msg: string) => message.warning(msg),
  },
  t,
  expandedKeys,
  scratchpadPath,
  fileMeta,
  newRefAlias,
  newRefPath,
  showRefModal,
  showPromoteConfirm,
  promoteTarget,
  showConflictDialog,
  conflictFilePath,
})
const {
  doImportFile,
  doEmptyTrash,
  doPromote,
  doUndoDelete,
  doRename,
  doAddReference,
  doRemoveReference,
  doBrowseRefPath,
} = fileActionsComposable

const inlineCreateParentPath = ref<string | null>(null)
const inlineCreateTargetDir = ref<string | null>(null)
const inlineCreateIsFolder = ref(false)
const isInlineCreateActive = ref(false)
const rootInlineInputRef = ref<HTMLInputElement | null>(null)
const rootInlineCreateName = ref('')
const rootInlineCreating = ref(false)

function toggleGroup(group: 'external' | 'local'): void {
  groupExpanded[group] = !groupExpanded[group]
}

function toggleTrash(): void {
  showTrash.value = !showTrash.value
  if (showTrash.value) {
    loadTrashEntries()
  }
}

const expandedSections = computed(() => {
  const sections: string[] = []
  if (groupExpanded.local) sections.push('local')
  if (groupExpanded.external) sections.push('external')
  if (showTrash.value) sections.push('trash')
  return sections
})

function getSectionFlexStyle(section: string): Record<string, string> {
  const expanded = expandedSections.value
  if (!expanded.includes(section)) {
    return { flex: '0 0 auto' }
  }
  if (expanded.length === 1) {
    return { flex: '1 1 0', minHeight: '0' }
  }
  const flex = sectionFlex[section] || 1
  return { flex: `${flex} 1 0`, minHeight: '40px' }
}

function startResize(section: string, event: MouseEvent): void {
  event.preventDefault()
  const expanded = expandedSections.value
  const idx = expanded.indexOf(section)
  if (idx < 0 || idx >= expanded.length - 1) return
  const current = expanded[idx]
  const next = expanded[idx + 1]
  resizing.value = section
  resizeStartY.value = event.clientY
  resizeStartFlex.value = [sectionFlex[current] || 1, sectionFlex[next] || 1]
  document.addEventListener('mousemove', handleResizeMove)
  document.addEventListener('mouseup', handleResizeEnd)
}

function handleResizeMove(event: MouseEvent): void {
  if (!resizing.value) return
  const expanded = expandedSections.value
  const idx = expanded.indexOf(resizing.value)
  if (idx < 0 || idx >= expanded.length - 1) return
  const current = expanded[idx]
  const next = expanded[idx + 1]
  const delta = event.clientY - resizeStartY.value
  const total = resizeStartFlex.value[0] + resizeStartFlex.value[1]
  if (total === 0) return
  const container = document.querySelector('.tree-container')
  const containerHeight = container ? container.clientHeight : 400
  if (containerHeight === 0) return
  const deltaRatio = (delta / containerHeight) * 100
  let newFlex0 = resizeStartFlex.value[0] + deltaRatio
  let newFlex1 = resizeStartFlex.value[1] - deltaRatio
  newFlex0 = Math.max(10, Math.min(90, newFlex0))
  newFlex1 = Math.max(10, Math.min(90, newFlex1))
  sectionFlex[current] = newFlex0
  sectionFlex[next] = newFlex1
}

function handleResizeEnd(): void {
  resizing.value = null
  document.removeEventListener('mousemove', handleResizeMove)
  document.removeEventListener('mouseup', handleResizeEnd)
}

function handleSelect(entry: ScratchpadEntry, event?: MouseEvent): void {
  const ctrl = event?.ctrlKey || event?.metaKey
  const shift = event?.shiftKey

  if (ctrl) {
    const next = new Set(selectedKeys.value)
    if (next.has(entry.path)) {
      next.delete(entry.path)
    } else {
      next.add(entry.path)
    }
    selectedKeys.value = next
    selectedKey.value = entry.path
    lastSelectPath.value = entry.path
    return
  }

  if (shift && lastSelectPath.value) {
    const flatEntries = flattenedTree.value.map(item => item.entry)
    const startIdx = flatEntries.findIndex(e => e.path === lastSelectPath.value)
    const endIdx = flatEntries.findIndex(e => e.path === entry.path)
    if (startIdx !== -1 && endIdx !== -1) {
      const from = Math.min(startIdx, endIdx)
      const to = Math.max(startIdx, endIdx)
      const next = new Set<string>()
      for (let i = from; i <= to; i++) {
        next.add(flatEntries[i].path)
      }
      selectedKeys.value = next
      selectedKey.value = entry.path
    }
    return
  }

  selectedKey.value = entry.path
  selectedKeys.value = new Set<string>()
  lastSelectPath.value = entry.path
}

async function handleToggleExpand(entry: ScratchpadEntry): Promise<void> {
  const normalizedPath = normalizePathForCompare(entry.path)
  const next = new Set(expandedKeys.value)
  if (next.has(normalizedPath)) {
    next.delete(normalizedPath)
    expandedKeys.value = next
    return
  }
  if (entry.kind === 'folder' && !hasChildrenLoaded(entry)) {
    await loadChildEntries(entry.path)
  }
  next.add(normalizedPath)
  expandedKeys.value = next
}

const dragEntry = ref<ScratchpadEntry | null>(null)

function handleTreeNodeDragStart(event: DragEvent, entry: ScratchpadEntry): void {
  dragEntry.value = entry
  const scratchpadBase = scratchpadPath.value || ''
  const relativePath = scratchpadBase
    ? entry.path.replace(scratchpadBase.replace(/\\/g, '/'), '').replace(/^\//, '')
    : entry.path
  if (event.dataTransfer) {
    event.dataTransfer.setData('application/x-scratchpad-file', relativePath)
    event.dataTransfer.setData('text/plain', relativePath)
    event.dataTransfer.effectAllowed = 'copy'
  }
}

async function handleNodeDrop(fromRelPath: string, toAbsPath: string): Promise<void> {
  const scratchpadBase = scratchpadPath.value || ''
  const toParent = toAbsPath
    .replace(scratchpadBase.replace(/\\/g, '/'), '')
    .replace(/^\//, '')
    .replace(/\\/g, '/')
  if (!fromRelPath || !toParent) return
  const entry = await moveEntry(fromRelPath, toParent)
  if (entry) {
    expandedKeys.value = new Set([...expandedKeys.value, toAbsPath])
    message.success(t('scratchpad.movedSuccess', { name: entry.name }))
  }
}

function collectFolderPaths(entries: ScratchpadEntry[]): string[] {
  const result: string[] = []
  for (const e of entries) {
    if (e.kind === 'folder') result.push(e.path)
    if (e.children) {
      result.push(...collectFolderPaths(e.children))
    }
  }
  return result
}

async function handleExpandAll(): Promise<void> {
  const allFolders = collectFolderPaths(filteredLocalEntries.value)
  expandedKeys.value = new Set(allFolders)
  for (const path of allFolders) {
    const entry = findEntryInTree(localEntries.value, path)
    if (entry && entry.kind === 'folder' && !hasChildrenLoaded(entry)) {
      await loadChildEntries(path)
    }
  }
}

function handleCollapseAll(): void {
  expandedKeys.value = new Set()
}

function handleOpen(entry: ScratchpadEntry): void {
  closeContextMenu()
  if (entry.kind === 'folder') {
    handleToggleExpand(entry)
    return
  }
  openFileInEditor(entry)
}

async function openFileInEditor(entry: ScratchpadEntry): Promise<void> {
  const ext = entry.name.includes('.') ? '.' + entry.name.split('.').pop()?.toLowerCase() : ''
  const langMap: Record<string, string> = {
    '.sql': 'sql',
    '.py': 'python',
    '.json': 'json',
    '.txt': 'plaintext',
    '.md': 'markdown',
  }
  const language = langMap[ext] || 'plaintext'

  const scratchpadBase = scratchpadPath.value || ''
  const relativePath = scratchpadBase
    ? entry.path.replace(scratchpadBase.replace(/\\/g, '/'), '').replace(/^\//, '')
    : entry.path

  let content: string | null = null
  try {
    content = await loadFileContent(relativePath)
  } catch {
    content = null
  }

  if (content === null) {
    content = ''
  }

  setContentSnapshot(relativePath, content)

  const metaForFile = fileMeta.value[relativePath]
  const lastConnectionId = metaForFile?.last_connection_id || ''

  window.dispatchEvent(
    new CustomEvent('open-sql-editor', {
      detail: {
        connectionId: lastConnectionId || '',
        databaseName: '',
        sql: content,
        scratchpadRelativePath: relativePath,
        scratchpadFileName: entry.name,
        language,
      },
    })
  )
  addRecentFile(relativePath)
}

async function openFileAtLine(relativePath: string, line: number): Promise<void> {
  const entry = localEntries.value.find(e => {
    const scratchpadBase = scratchpadPath.value || ''
    const relPath = scratchpadBase
      ? e.path.replace(scratchpadBase.replace(/\\/g, '/'), '').replace(/^\//, '')
      : e.path
    return relPath === relativePath
  })
  if (!entry) return

  const ext = entry.name.includes('.') ? '.' + entry.name.split('.').pop()?.toLowerCase() : ''
  const langMap: Record<string, string> = {
    '.sql': 'sql',
    '.py': 'python',
    '.json': 'json',
    '.txt': 'plaintext',
    '.md': 'markdown',
  }
  const language = langMap[ext] || 'plaintext'

  const content = await loadFileContent(relativePath)
  if (content === null) return

  setContentSnapshot(relativePath, content)

  const metaForFile = fileMeta.value[relativePath]
  const lastConnectionId = metaForFile?.last_connection_id || ''

  window.dispatchEvent(
    new CustomEvent('open-sql-editor', {
      detail: {
        connectionId: lastConnectionId || '',
        databaseName: '',
        sql: content,
        scratchpadRelativePath: relativePath,
        scratchpadFileName: entry.name,
        language,
        initialLine: line,
      },
    })
  )
  addRecentFile(relativePath)
}

function escapeHtml(str: string): string {
  return str
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;')
}

let rootInlineClickCleanup: (() => void) | null = null
let rootInlineClickTimer: ReturnType<typeof setTimeout> | null = null

function registerRootInlineClickOutside(): void {
  cleanupRootInlineClickOutside()
  const handler = (event: MouseEvent): void => {
    const target = event.target as HTMLElement
    if (target.closest('.inline-tree-entry')) {
      document.addEventListener('click', handler, { once: true })
      return
    }
    commitRootInlineCreate()
  }
  rootInlineClickTimer = setTimeout(() => {
    rootInlineClickTimer = null
    document.addEventListener('click', handler, { once: true })
  }, 0)
  rootInlineClickCleanup = () => {
    if (rootInlineClickTimer !== null) {
      clearTimeout(rootInlineClickTimer)
      rootInlineClickTimer = null
    }
    document.removeEventListener('click', handler)
  }
}

function cleanupRootInlineClickOutside(): void {
  if (rootInlineClickCleanup) {
    rootInlineClickCleanup()
    rootInlineClickCleanup = null
  }
}

function startInlineCreate(parentPath: string | null, isFolder: boolean): void {
  // eslint-disable-next-line no-console
  console.log('[Scratchpad] startInlineCreate, parentPath:', parentPath, 'isFolder:', isFolder)
  showExplorerFilter.value = false
  contentSearchMode.value = false
  groupExpanded.local = true
  isInlineCreateActive.value = true
  inlineCreateIsFolder.value = isFolder
  rootInlineCreateName.value = ''

  if (!parentPath) {
    inlineCreateParentPath.value = null
    inlineCreateTargetDir.value = null
    nextTick(() => {
      rootInlineInputRef.value?.scrollIntoView({ block: 'nearest' })
      rootInlineInputRef.value?.focus()
      registerRootInlineClickOutside()
    })
    return
  }

  const entry = findEntryInTree(localEntries.value, parentPath)
  // eslint-disable-next-line no-console
  console.log(
    '[Scratchpad] startInlineCreate: findEntryInTree found?',
    !!entry,
    'kind:',
    entry?.kind
  )
  if (entry && entry.kind === 'folder') {
    inlineCreateParentPath.value = parentPath
    inlineCreateTargetDir.value = parentPath
    const normalized = normalizePathForCompare(parentPath)
    expandedKeys.value = new Set([...expandedKeys.value, normalized])
    if (!hasChildrenLoaded(entry)) {
      loadChildEntries(parentPath)
    }
  } else {
    inlineCreateParentPath.value = null
    inlineCreateTargetDir.value = parentPath
    nextTick(() => {
      rootInlineInputRef.value?.scrollIntoView({ block: 'nearest' })
      rootInlineInputRef.value?.focus()
      registerRootInlineClickOutside()
    })
  }
}

function cancelInlineCreate(): void {
  isInlineCreateActive.value = false
  inlineCreateParentPath.value = null
  inlineCreateTargetDir.value = null
  inlineCreateIsFolder.value = false
  rootInlineCreateName.value = ''
  rootInlineCreating.value = false
  cleanupRootInlineClickOutside()
}

function commitRootInlineCreate(_event?: FocusEvent): void {
  if (rootInlineCreating.value) return
  const name = rootInlineCreateName.value.trim()
  // eslint-disable-next-line no-console
  console.log('[Scratchpad] commitRootInlineCreate, name:', name)
  if (!name) {
    cancelInlineCreate()
    return
  }
  rootInlineCreating.value = true
  rootInlineCreateName.value = ''
  confirmInlineCreate(name)
}

function cancelRootInlineCreate(): void {
  rootInlineCreating.value = false
  cancelInlineCreate()
}

async function confirmInlineCreate(name: string): Promise<void> {
  const parentPath = inlineCreateTargetDir.value || inlineCreateParentPath.value
  const isFolder = inlineCreateIsFolder.value
  // eslint-disable-next-line no-console
  console.log(
    '[Scratchpad] confirmInlineCreate, name:',
    name,
    'isFolder:',
    isFolder,
    'parentPath:',
    parentPath,
    'inlineCreateTargetDir:',
    inlineCreateTargetDir.value,
    'inlineCreateParentPath:',
    inlineCreateParentPath.value
  )
  if (!name) {
    cancelInlineCreate()
    return
  }
  cancelInlineCreate()
  const entry = await createEntry(name, isFolder, parentPath || undefined)
  if (entry) {
    if (parentPath) {
      expandedKeys.value = new Set([...expandedKeys.value, parentPath])
    }
    message.success(t('scratchpad.createdSuccess', { name }))
    if (!isFolder) {
      openFileInEditor(entry)
    }
  }
}

async function handleCreateFile(): Promise<void> {
  // eslint-disable-next-line no-console
  console.log('[Scratchpad] handleCreateFile 被点击')
  const selectedFolder = findSelectedFolder()
  startInlineCreate(selectedFolder, false)
}

async function handleCreateFolder(): Promise<void> {
  // eslint-disable-next-line no-console
  console.log('[Scratchpad] handleCreateFolder 被点击')
  const selectedFolder = findSelectedFolder()
  startInlineCreate(selectedFolder, true)
}

function findSelectedFolder(): string | null {
  const sel = selectedKey.value
  if (!sel) return null
  const entry = findEntryInTree(localEntries.value, sel)
  if (entry && entry.kind === 'folder') return sel
  if (entry) {
    return getParentPathOfEntry(sel)
  }
  return null
}

function findEntryInTree(entries: ScratchpadEntry[], path: string): ScratchpadEntry | undefined {
  for (const e of entries) {
    if (e.path === path) return e
    if (e.children) {
      const found = findEntryInTree(e.children, path)
      if (found) return found
    }
  }
  return undefined
}

function getParentPathOfEntry(path: string): string | null {
  const normalized = path.replace(/\\/g, '/')
  const lastSlash = normalized.lastIndexOf('/')
  return lastSlash > 0 ? normalized.substring(0, lastSlash) : null
}

async function handleImportFile(): Promise<void> {
  await doImportFile()
}

async function handleRestore(name: string): Promise<void> {
  await doUndoDelete(name)
}

const isDragOver = ref(false)
let dragLeaveTimer: ReturnType<typeof setTimeout> | null = null

function handleDragOver(_event: DragEvent): void {
  if (dragLeaveTimer) {
    clearTimeout(dragLeaveTimer)
    dragLeaveTimer = null
  }
  isDragOver.value = true
}

function handleDragLeave(_event: DragEvent): void {
  dragLeaveTimer = setTimeout(() => {
    isDragOver.value = false
  }, 100)
}

async function handleDrop(event: DragEvent): Promise<void> {
  isDragOver.value = false
  const files = event.dataTransfer?.files
  if (!files || files.length === 0) return

  for (let i = 0; i < files.length; i++) {
    const filePath = (files[i] as unknown as { path?: string }).path
    if (filePath) {
      const result = await importFile(filePath)
      if (result) message.success(t('scratchpad.importedSuccess', { name: result.name }))
    }
  }
}

async function handleAddReference(): Promise<void> {
  newRefAlias.value = ''
  newRefPath.value = ''
  showRefModal.value = true
}

async function handleSaveAll(): Promise<void> {
  message.info(t('scratchpad.saveAllHint'))
}

async function handleCloseRecent(): Promise<void> {
  recentFiles.value = []
  message.success(t('scratchpad.recentCleared'))
}

async function handleEmptyTrash(): Promise<void> {
  await doEmptyTrash()
}

async function browseRefPath(): Promise<void> {
  await doBrowseRefPath()
}

async function confirmAddReference(): Promise<void> {
  const alias = newRefAlias.value.trim()
  const path = newRefPath.value.trim()
  if (!alias || !path) return
  await doAddReference(alias, path)
}

function handleRefClick(ref: ExternalReference): void {
  selectedKey.value = ref.alias
}

async function handleAnalyzeDuckDB(entry: ScratchpadEntry): Promise<void> {
  await loadAnalyzableFiles()
  const match = analyzableFiles.value.find(f => f.relative_path === entry.path)
  if (!match) return

  const content = match.duckdb_query_hint

  window.dispatchEvent(
    new CustomEvent('open-sql-editor', {
      detail: {
        connectionId: '',
        databaseName: '',
        sql: content,
        scratchpadRelativePath: '',
        scratchpadFileName: `${entry.name} (DuckDB Preview)`,
        language: 'sql',
      },
    })
  )
}

async function handlePaste(): Promise<void> {
  const src = clipboardEntry.value
  if (!src) return

  const ctxTarget = contextMenu.target as ScratchpadEntry | null
  const targetFolderPath = ctxTarget?.kind === 'folder' ? ctxTarget.path : findSelectedFolder()

  if (clipboardMode.value === 'cut') {
    const fromRelPath = src.path
      .replace(scratchpadPath.value || '', '')
      .replace(/^\//, '')
      .replace(/\\/g, '/')
    const toParent = targetFolderPath
      ? targetFolderPath
          .replace(scratchpadPath.value || '', '')
          .replace(/^\//, '')
          .replace(/\\/g, '/')
      : ''
    const entry = await moveEntry(fromRelPath, toParent)
    if (entry) {
      clipboardEntry.value = null
      clipboardMode.value = 'copy'
      showMoveUndo(fromRelPath, toParent, entry.name)
      message.success(t('scratchpad.movedSuccess', { name: entry.name }))
    }
    return
  }

  const content = await loadFileContent(
    src.path.replace(scratchpadPath.value || '', '').replace(/^\//, '')
  )
  if (content === null) return
  const baseName = src.name.replace(/\.([^.]+)$/, '')
  const ext = src.name.includes('.') ? '.' + src.name.split('.').pop() : ''
  const copyName = `${baseName}_copy${ext}`
  const entry = await createEntry(copyName, false)
  if (entry) {
    const relPath = entry.path.replace(scratchpadPath.value || '', '').replace(/^\//, '')
    await saveFile(relPath, content)
    message.success(t('scratchpad.pasteCopied', { name: copyName }))
  }
}

function showMoveUndo(fromPath: string, toParent: string, name: string): void {
  dismissMoveUndo()
  moveUndoVisible.value = true
  moveUndoFromPath.value = fromPath
  moveUndoToParent.value = toParent
  moveUndoName.value = name
  moveUndoTimer = setTimeout(() => {
    dismissMoveUndo()
  }, MOVE_UNDO_TIMEOUT)
}

function dismissMoveUndo(): void {
  moveUndoVisible.value = false
  if (moveUndoTimer) {
    clearTimeout(moveUndoTimer)
    moveUndoTimer = null
  }
}

async function handleMoveUndo(): Promise<void> {
  const fromPath = moveUndoFromPath.value
  const toParent = moveUndoToParent.value
  dismissMoveUndo()
  const entry = await moveEntry(
    `${toParent ? toParent + '/' : ''}${moveUndoName.value}`,
    getParentPathOfEntry(fromPath) || ''
  )
  if (entry) {
    message.success(t('scratchpad.undoMoveSuccess', { name: entry.name }))
  }
}

async function handlePromoteConfirm(removeAfter: boolean): Promise<void> {
  showPromoteConfirm.value = false
  const entry = promoteTarget.value
  if (!entry) return
  await doPromote(entry, removeAfter)
  promoteTarget.value = null
}

function handleConflictReload(): void {
  const path = conflictFilePath.value
  showConflictDialog.value = false
  conflictFilePath.value = null
  if (path) {
    markClean(path)
    dismissConflict(path)
    handleOpenByPath(path)
  }
}

function handleConflictIgnore(): void {
  const path = conflictFilePath.value
  showConflictDialog.value = false
  conflictFilePath.value = null
  if (path) dismissConflict(path)
}

async function handleConflictDiff(): Promise<void> {
  const path = conflictFilePath.value
  if (!path) return
  showConflictDialog.value = false
  try {
    const entry = findEntryInTree(localEntries.value, path)
    if (!entry) return
    const editorSnapshot = getContentSnapshot(path)
    if (editorSnapshot === null) {
      handleConflictReload()
      return
    }
    const result = await diffWithContent(
      path,
      editorSnapshot,
      t('scratchpad.diffCurrentFile'),
      t('scratchpad.diffEditedContent')
    )
    if (result) {
      diffResult.value = result
      diffFilePath.value = path
      diffLeftLabel.value = t('scratchpad.diffCurrentFile')
      diffRightLabel.value = t('scratchpad.diffEditedContent')
      showDiffDialog.value = true
    }
  } catch {
    handleConflictReload()
  }
}

async function handleDiffAcceptRight(): Promise<void> {
  const path = diffFilePath.value
  showDiffDialog.value = false
  if (!path) return
  markClean(path)
  dismissConflict(path)
  handleOpenByPath(path)
}

function handleDiffClose(): void {
  showDiffDialog.value = false
  if (diffFilePath.value) {
    showConflictDialog.value = true
  }
}

async function handleOpenByPath(path: string): Promise<void> {
  const entry = findEntryInTree(localEntries.value, path)
  if (entry && entry.kind === 'file') {
    handleOpen(entry)
  }
}

watch(externalConflicts, newConflicts => {
  if (newConflicts.length > 0 && !showConflictDialog.value) {
    conflictFilePath.value = newConflicts[0]
    showConflictDialog.value = true
  }
})

function startRename(entry: ScratchpadEntry): void {
  renamingKey.value = entry.path
}

async function finishRename(entry: ScratchpadEntry, newName: string): Promise<void> {
  renamingKey.value = null
  if (newName && newName !== entry.name) {
    await doRename(entry.path, newName)
  }
}

function cancelRename(): void {
  renamingKey.value = null
}

function scrollToSelected(): void {
  nextTick(() => {
    const el = document.querySelector('.node-row.selected')
    el?.scrollIntoView({ block: 'nearest' })
  })
}

let unlisten: (() => void) | null = null
let resizeObserver: ResizeObserver | null = null

onMounted(async () => {
  await loadFiles()
  await nextTick()
  updateTreeContainerSize()
  resizeObserver = new ResizeObserver(() => updateTreeContainerSize())
  if (treeContainerRef.value) {
    resizeObserver.observe(treeContainerRef.value)
  }
  document.addEventListener('click', closeContextMenu)
  scratchpadPanelRef.value?.addEventListener('keydown', handleKeydown)
  window.addEventListener('project-switched', loadFiles)
  await startWatching()
  try {
    const unlistenFn = await listen<ScratchpadChangeEvent>('scratchpad-changed', event => {
      const payload = event.payload
      if (!payload.changes || payload.changes.length === 0) {
        return
      }
      const savedExpanded = new Set(expandedKeys.value)
      const savedSelected = selectedKey.value
      applyFileChanges(payload).then(() => {
        expandedKeys.value = savedExpanded
        selectedKey.value = savedSelected
      })
    })
    unlisten = unlistenFn
  } catch {
    // event listener setup failed, watcher still works for in-app changes
  }
})

onUnmounted(() => {
  resizeObserver?.disconnect()
  document.removeEventListener('click', closeContextMenu)
  scratchpadPanelRef.value?.removeEventListener('keydown', handleKeydown)
  window.removeEventListener('project-switched', loadFiles)
  if (unlisten) {
    unlisten()
    unlisten = null
  }
  stopWatching()
})
</script>

<style scoped>
.scratchpad-app-wrapper {
  height: 100%;
  display: flex;
  flex-direction: column;
}

:deep(.n-config-provider),
:deep(.n-message-provider) {
  height: 100%;
  display: flex;
  flex-direction: column;
}

.scratchpad-panel {
  display: flex;
  flex-direction: column;
  flex: 1;
  min-height: 0;
  background: var(--dv-background-color, var(--color-bg-primary));
  overflow: hidden;
  position: relative;
}

.scratchpad-panel.dragover-active::after {
  content: attr(data-drop-hint);
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--brand-accent-soft);
  border: 2px dashed var(--brand-accent);
  color: var(--brand-accent);
  font-size: var(--font-size-lg);
  font-weight: 600;
  z-index: 100;
  pointer-events: none;
}

.explorer-titlebar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  height: var(--title-bar-height, 36px);
  padding: 0 var(--spacing-sm) 0 var(--spacing-md);
  flex-shrink: 0;
  border-bottom: 1px solid var(--color-border-subtle);
  position: relative;
  z-index: 1;
}

.explorer-titlebar-label {
  margin: 0;
  font-size: var(--font-size-sm);
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--color-text-secondary);
  user-select: none;
}

.explorer-titlebar-actions {
  display: flex;
  align-items: center;
  gap: var(--spacing-xs);
}

.scratchpad-toolbar {
  display: flex;
  align-items: center;
  padding: 0 var(--spacing-xs);
  background: var(--color-bg-secondary);
  border-bottom: 1px solid var(--color-border-subtle);
  gap: var(--spacing-xs);
  min-height: 28px;
  flex-shrink: 0;
}

.scratchpad-explorer-body {
  flex: 1;
  display: flex;
  flex-direction: column;
  overflow: visible;
  min-height: 0;
  background: var(--dv-background-color, var(--color-bg-primary));
}

.toolbar-btn {
  width: 26px;
  height: 26px;
  border: none;
  border-radius: var(--border-radius-sm);
  background: transparent;
  color: var(--color-text-secondary);
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  transition:
    background 0.1s ease,
    color 0.1s ease;
  padding: 0;
  outline: none;
}

.toolbar-btn:focus-visible {
  box-shadow: 0 0 0 1px var(--primary-color);
  z-index: 1;
}

.toolbar-btn:hover:not(:disabled) {
  background: var(--color-hover);
  color: var(--color-text-primary);
}

.toolbar-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.toolbar-btn-primary {
  color: var(--color-text-primary);
}

.toolbar-btn-active {
  background: var(--color-hover);
  color: var(--color-text-primary);
}

.toolbar-separator {
  width: 1px;
  height: 16px;
  background: var(--color-border-subtle);
  margin: 0 var(--spacing-xs);
}

.icon-spin {
  animation: toolbar-spin 0.8s linear infinite;
}

@keyframes toolbar-spin {
  from {
    transform: rotate(0deg);
  }
  to {
    transform: rotate(360deg);
  }
}

.search-panel {
  flex: 1;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  background: var(--color-bg-primary);
}

.search-panel-header {
  display: flex;
  align-items: center;
  gap: var(--spacing-xs);
  padding: var(--spacing-xs) var(--spacing-sm);
  border-bottom: 1px solid var(--color-border-subtle);
  flex-shrink: 0;
}

.search-input-wrapper {
  flex: 1;
  display: flex;
  align-items: center;
  gap: var(--spacing-xs);
}

.search-panel-input {
  flex: 1;
}

.search-panel-input :deep(.n-input) {
  --n-border: var(--color-border-subtle);
  --n-border-hover: var(--primary-color);
  --n-border-focus: var(--primary-color);
  --n-color: var(--color-bg-tertiary);
  --n-color-focus: var(--color-bg-primary);
  --n-text-color: var(--color-text-primary);
  --n-placeholder-color: var(--color-text-muted);
  --n-border-radius: var(--border-radius-sm);
  --n-caret-color: var(--color-text-primary);
}

.search-panel-input :deep(.n-input .n-input__input-el),
.search-panel-input :deep(.n-input__input-el),
.search-panel-input :deep(.n-input__input),
.search-panel-input :deep(.n-input input) {
  background-color: transparent !important;
  color: var(--color-text-primary) !important;
}

.search-toggle-buttons {
  display: flex;
  align-items: center;
  gap: var(--spacing-xs);
}

.toggle-btn {
  height: 22px;
  padding: 0 var(--spacing-xs);
  border: none;
  background: transparent;
  color: var(--color-text-muted);
  font-size: var(--font-size-xs);
  font-weight: 600;
  cursor: pointer;
  border-radius: var(--border-radius-sm);
  display: flex;
  align-items: center;
  justify-content: center;
  transition:
    background 0.15s,
    color 0.15s;
}

.toggle-btn:hover {
  background: var(--color-hover);
  color: var(--color-text-secondary);
}

.toggle-btn-active {
  background: var(--brand-accent-soft);
  color: var(--brand-accent);
}

.search-filter-toggle {
  display: flex;
  align-items: center;
  gap: var(--spacing-xs);
  padding: var(--spacing-xs) var(--spacing-sm);
  cursor: pointer;
  font-size: var(--font-size-xs);
  color: var(--color-text-muted);
  user-select: none;
  transition: color 0.15s;
}

.search-filter-toggle:hover {
  color: var(--color-text-secondary);
}

.search-filters {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-xs);
  padding: 0 var(--spacing-sm) var(--spacing-xs);
}

.search-filter-input {
  width: 100%;
}

.search-filter-input :deep(.n-input) {
  --n-border: var(--color-border-subtle);
  --n-border-hover: var(--primary-color);
  --n-border-focus: var(--primary-color);
  --n-color: var(--color-bg-tertiary);
  --n-color-focus: var(--color-bg-primary);
  --n-text-color: var(--color-text-primary);
  --n-placeholder-color: var(--color-text-muted);
  --n-border-radius: var(--border-radius-sm);
  --n-caret-color: var(--color-text-primary);
}

.search-filter-input :deep(.n-input .n-input__input-el),
.search-filter-input :deep(.n-input__input-el),
.search-filter-input :deep(.n-input__input),
.search-filter-input :deep(.n-input input) {
  background-color: transparent !important;
  color: var(--color-text-primary) !important;
}

.search-replace-bar {
  display: flex;
  align-items: center;
  gap: var(--spacing-xs);
  padding: var(--spacing-xs) var(--spacing-sm);
  border-top: 1px solid var(--color-border-subtle);
}

.search-replace-input {
  flex: 1;
}

.search-replace-input :deep(.n-input) {
  --n-border: var(--color-border-subtle);
  --n-border-hover: var(--primary-color);
  --n-border-focus: var(--primary-color);
  --n-color: var(--color-bg-tertiary);
  --n-color-focus: var(--color-bg-primary);
  --n-text-color: var(--color-text-primary);
  --n-placeholder-color: var(--color-text-muted);
  --n-border-radius: var(--border-radius-sm);
  --n-caret-color: var(--color-text-primary);
}

.search-replace-input :deep(.n-input .n-input__input-el),
.search-replace-input :deep(.n-input__input-el),
.search-replace-input :deep(.n-input__input),
.search-replace-input :deep(.n-input input) {
  background-color: transparent !important;
  color: var(--color-text-primary) !important;
}

.search-results {
  flex: 1;
  overflow-y: auto;
  padding: var(--spacing-xs) 0;
}

.search-result-group {
  margin-bottom: 1px;
}

.search-result-file-header {
  display: flex;
  align-items: center;
  gap: var(--spacing-xs);
  height: 22px;
  padding: 0 var(--spacing-sm);
  cursor: pointer;
  user-select: none;
  transition: background-color 0.15s;
}

.search-result-file-header:hover {
  background-color: var(--color-hover);
}

.search-result-file-name {
  flex: 1;
  font-size: var(--font-size-md);
  color: var(--color-text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.search-result-match-count {
  font-size: var(--font-size-xs);
  color: var(--color-text-muted);
  background: var(--color-bg-tertiary);
  padding: 0 var(--spacing-xs);
  border-radius: var(--border-radius-sm);
}

.search-result-matches {
  padding: 0;
}

.search-result-line {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
  height: 22px;
  padding: 0 var(--spacing-sm) 0 var(--spacing-xl);
  cursor: pointer;
  transition: background-color 0.15s;
}

.search-result-line:hover {
  background-color: var(--color-hover);
}

.search-result-line-number {
  font-size: var(--font-size-xs);
  color: var(--color-text-muted);
  min-width: 28px;
  text-align: right;
  font-family: var(--font-monospace);
  flex-shrink: 0;
}

.search-result-line-content {
  font-size: var(--font-size-md);
  font-family: var(--font-monospace);
  color: var(--color-text-secondary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  flex: 1;
}

.search-results-footer {
  padding: var(--spacing-xs) var(--spacing-sm);
  font-size: var(--font-size-xs);
  color: var(--color-text-muted);
  border-top: 1px solid var(--color-border-subtle);
}

.search-hl {
  background: var(--brand-accent-soft);
  color: var(--brand-accent);
  border-radius: var(--border-radius-sm);
}

.search-notice {
  display: flex;
  align-items: center;
  gap: var(--spacing-xs);
  padding: var(--spacing-xs) var(--spacing-sm);
  font-size: var(--font-size-xs);
}

.search-notice-error {
  color: var(--brand-danger);
  background: color-mix(in srgb, var(--brand-danger) 10%, transparent);
}

.search-no-results {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: var(--spacing-lg);
  font-size: var(--font-size-md);
  color: var(--color-text-muted);
}

.inline-tree-entry {
  display: flex;
  align-items: center;
  gap: var(--spacing-xs);
  height: 22px;
  padding: 0 var(--spacing-sm) 0 var(--spacing-lg);
}

.inline-tree-entry .rename-input {
  flex: 1;
  height: 18px;
  padding: 0 var(--spacing-xs);
  font-size: var(--font-size-md);
  font-family: var(--font-family);
  border: 1px solid var(--primary-color);
  border-radius: var(--border-radius-sm);
  outline: none;
  background: var(--color-bg-primary);
  color: var(--color-text-primary);
}

.tree-container {
  flex: 1;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  min-height: 0;
}

.virtual-scroll-viewport {
  position: relative;
  width: 100%;
}

.virtual-scroll-spacer {
  width: 100%;
}

.loading-state,
.error-state {
  display: flex;
  align-items: center;
  justify-content: center;
  padding: var(--spacing-xl);
  gap: var(--spacing-sm);
}

.error-text {
  color: var(--brand-danger);
  font-size: var(--font-size-md);
}

.pane-section {
  display: flex;
  flex-direction: column;
  min-height: 0;
  border-top: 1px solid var(--color-border-subtle);
  overflow: hidden;
}

.pane-section:first-child {
  border-top: none;
}

.pane-section-main {
  min-height: 0;
}

.pane-section-main .pane-section-body {
  flex: 1;
  overflow-y: auto;
  min-height: 0;
}

.pane-section-header {
  display: flex;
  align-items: center;
  gap: var(--spacing-xs);
  height: 26px;
  padding: 0 var(--spacing-sm) 0 var(--spacing-sm);
  cursor: pointer;
  font-size: var(--font-size-xs);
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.025em;
  color: var(--color-text-secondary);
  transition:
    background-color 0.15s,
    color 0.15s;
  user-select: none;
  position: relative;
  z-index: 2;
}

.pane-section-header:hover {
  background-color: var(--color-hover);
}

.pane-section-title {
  margin: 0;
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.pane-section-actions {
  display: flex;
  gap: var(--spacing-xs);
  margin-left: auto;
  font-size: var(--font-size-xs);
  opacity: 0.55;
  transition: opacity 0.15s;
  pointer-events: auto;
}

.pane-section-actions .toolbar-btn {
  pointer-events: auto;
}

.pane-section-header:hover .pane-section-actions {
  opacity: 1;
}

.pane-section-actions .n-button {
  font-size: var(--font-size-xs);
  padding: 0 var(--spacing-xs);
  height: 20px;
}

.pane-section-body {
  padding: 0;
}

.section-resize-handle {
  height: var(--spacing-xs);
  cursor: row-resize;
  background: transparent;
  transition: background 0.15s;
  flex-shrink: 0;
  position: relative;
}

.section-resize-handle:hover,
.section-resize-active {
  background: var(--primary-color);
}

.ref-row {
  display: flex;
  align-items: center;
  gap: var(--spacing-xs);
  height: 22px;
  padding: 0 var(--spacing-sm) 0 var(--spacing-lg);
  font-size: var(--font-size-md);
  cursor: pointer;
  transition: background-color 0.15s;
}

.ref-row:hover {
  background-color: var(--color-hover);
}

.ref-name {
  color: var(--color-text-primary);
  font-weight: 400;
}

.ref-path {
  font-size: var(--font-size-xs);
  color: var(--color-text-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  flex: 1;
}

.ref-path-invalid {
  color: var(--brand-danger);
  text-decoration: line-through;
}

.ref-badge {
  font-size: var(--font-size-xxs);
  color: var(--brand-danger);
  background: var(--brand-danger-soft);
  padding: 0 var(--spacing-xs);
  border-radius: var(--border-radius-sm);
  flex-shrink: 0;
}

.ref-invalid {
  color: var(--brand-danger);
}

.empty-hint {
  font-size: var(--font-size-md);
  color: var(--color-text-muted);
  padding: var(--spacing-sm) var(--spacing-lg);
}

.recent-section {
  border-bottom: 1px solid var(--color-border-subtle);
}

.recent-header {
  display: flex;
  align-items: center;
  gap: var(--spacing-xs);
  height: 22px;
  padding: 0 var(--spacing-md) 0 var(--spacing-lg);
  cursor: pointer;
  user-select: none;
}

.recent-header:hover {
  background-color: var(--color-hover);
}

.recent-title {
  font-size: var(--font-size-md);
  font-weight: 400;
  color: var(--color-text-secondary);
}

.recent-count {
  font-size: var(--font-size-xs);
  color: var(--color-text-muted);
}

.recent-list {
  padding-bottom: var(--spacing-xs);
}

.recent-entry {
  display: flex;
  align-items: center;
  gap: var(--spacing-xs);
  height: 22px;
  padding: 0 var(--spacing-sm) 0 var(--spacing-xl);
  cursor: pointer;
  font-size: var(--font-size-md);
  color: var(--color-text-primary);
  transition: background-color 0.15s;
}

.recent-entry:hover {
  background-color: var(--color-hover);
}

.recent-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: var(--font-size-md);
}

.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: var(--spacing-xl) var(--spacing-lg);
  gap: var(--spacing-sm);
}

.empty-icon-wrapper {
  color: var(--color-text-muted);
  opacity: 0.3;
  margin-bottom: var(--spacing-xs);
}

.empty-title {
  font-size: var(--font-size-md);
  font-weight: 400;
  color: var(--color-text-muted);
}

.empty-hint {
  font-size: var(--font-size-sm);
  color: var(--color-text-muted);
  padding: 0;
  text-align: center;
}

.empty-actions {
  display: flex;
  gap: var(--spacing-sm);
  margin-top: var(--spacing-sm);
}

.modal-field {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-xs);
}

.modal-label {
  font-size: var(--font-size-sm);
  font-weight: 500;
  color: var(--color-text-secondary);
  user-select: none;
}

.modal-input {
  width: 100%;
}

.modal-input-row {
  display: flex;
  gap: var(--spacing-sm);
  align-items: center;
}

.modal-input-row .modal-input {
  flex: 1;
}

.modal-actions {
  display: flex;
  justify-content: flex-end;
  gap: var(--spacing-sm);
}

.modal-header-row {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
}

.modal-header-title {
  font-size: var(--font-size-md);
  font-weight: 600;
  color: var(--color-text-primary);
}

/* ===== NaiveUI 组件主题适配 ===== */

:deep(.n-dropdown-menu) {
  --n-color: var(--color-bg-elevated) !important;
  --n-text-color: var(--color-text-primary) !important;
  --n-option-color-hover: var(--color-hover) !important;
  --n-option-text-color-hover: var(--color-text-primary) !important;
  --n-option-text-color-active: var(--color-text-primary) !important;
  --n-divider-color: var(--color-border-subtle) !important;
  --n-border-radius: var(--border-radius-md) !important;
  box-shadow: var(--shadow-dropdown) !important;
}

:deep(.n-dropdown-menu .n-dropdown-option) {
  font-size: var(--font-size-md) !important;
}

/* NModal preset="card" 主题适配 */
:deep(.n-modal .n-card) {
  --n-color: var(--color-bg-elevated) !important;
  --n-text-color: var(--color-text-primary) !important;
  --n-title-text-color: var(--color-text-primary) !important;
  --n-close-color: var(--color-text-muted) !important;
  --n-close-color-hover: var(--color-text-primary) !important;
  --n-border-color: var(--color-border-subtle) !important;
  --n-title-font-weight: 600 !important;
}

:deep(.n-modal .n-card > .n-card__content) {
  color: var(--color-text-primary) !important;
}

:deep(.n-modal .n-scrollbar-content) {
  --n-color: var(--color-bg-elevated) !important;
}

/* NModal segmented 区域 */
:deep(.n-modal .n-card > .n-card__footer) {
  --n-color: var(--color-bg-secondary) !important;
  border-top: 1px solid var(--color-border-subtle) !important;
}

:deep(.n-modal .n-card .n-card__content) {
  background: var(--color-bg-elevated) !important;
}

/* NModal 内嵌的 NSelect/NAutoComplete 组件 */
:deep(.n-modal .n-base-selection) {
  --n-color: var(--color-bg-tertiary) !important;
  --n-color-active: var(--color-bg-tertiary) !important;
  --n-border: var(--color-border-subtle) !important;
  --n-border-hover: var(--primary-color) !important;
  --n-border-focus: var(--primary-color) !important;
  --n-text-color: var(--color-text-primary) !important;
  --n-placeholder-color: var(--color-text-muted) !important;
}

/* NModal 内嵌的 NButton */
:deep(.n-modal .n-button) {
  --n-text-color: var(--color-text-primary) !important;
  --n-color: var(--color-bg-tertiary) !important;
  --n-color-hover: var(--color-hover) !important;
}

:deep(.n-modal .n-button--primary-type) {
  --n-text-color: #ffffff !important;
  --n-color: var(--brand-accent) !important;
  --n-color-hover: var(--brand-accent-hover) !important;
}

/* NScrollbar 主题适配 */
:deep(.n-scrollbar > .n-scrollbar-rail > .n-scrollbar-rail__scrollbar) {
  --n-color: var(--color-text-muted) !important;
  --n-color-hover: var(--color-text-secondary) !important;
}

/* 通用 NButton 面板内适配 */
:deep(
  .n-button:not(.n-button--primary-type):not(.n-button--error-type):not(
      .n-button--warning-type
    ):not(.n-button--info-type):not(.n-button--success-type)
) {
  --n-text-color: var(--color-text-primary) !important;
  --n-color-hover: var(--color-hover) !important;
  --n-border: var(--color-border) !important;
  --n-border-hover: var(--color-border) !important;
}

/* NCheckbox 适应 */
:deep(.n-checkbox .n-checkbox__label) {
  --n-text-color: var(--color-text-secondary) !important;
}

/* NEmpty 适应 */
:deep(.n-empty .n-empty__description) {
  --n-text-color: var(--color-text-muted) !important;
}

/* NSelect 下拉面板适配 */
:deep(.n-base-select-menu) {
  --n-color: var(--color-bg-elevated) !important;
  --n-text-color: var(--color-text-primary) !important;
  --n-option-color-hover: var(--color-hover) !important;
  --n-option-text-color-hover: var(--color-text-primary) !important;
  --n-divider-color: var(--color-border-subtle) !important;
  box-shadow: var(--shadow-dropdown) !important;
}

.scratchpad-context-menu {
  position: fixed;
  z-index: 99999;
  background: var(--color-bg-elevated);
  border: 1px solid var(--color-border);
  border-radius: var(--border-radius-md);
  box-shadow: var(--shadow-dropdown);
  min-width: 180px;
  padding: var(--spacing-xs) 0;
}

.context-menu-fade-enter-active,
.context-menu-fade-leave-active {
  transition: opacity 0.12s ease;
}

.context-menu-fade-enter-from,
.context-menu-fade-leave-to {
  opacity: 0;
}

.menu-item {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
  height: 24px;
  padding: 0 var(--spacing-md);
  font-size: var(--font-size-md);
  cursor: pointer;
  transition: background-color 0.15s;
  color: var(--color-text-primary);
}

.menu-item:hover {
  background-color: var(--color-hover);
}

.menu-item-danger {
  color: var(--brand-danger);
}

.menu-item-danger:hover {
  background-color: var(--brand-accent-soft);
}

.menu-item-disabled {
  opacity: 0.4;
  cursor: default;
}

.menu-item-disabled:hover {
  background-color: transparent;
}

.menu-shortcut {
  margin-left: auto;
  font-size: var(--font-size-xs);
  color: var(--color-text-muted);
}

.menu-item-danger .menu-shortcut {
  opacity: 0.6;
}

.menu-divider {
  height: 1px;
  background: var(--color-border-subtle);
  margin: var(--spacing-xs) var(--spacing-sm);
}

.undo-bar {
  position: absolute;
  bottom: 0;
  left: 0;
  right: 0;
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
  padding: var(--spacing-sm) var(--spacing-md);
  background: var(--color-bg-elevated);
  border-top: 1px solid var(--color-border);
  font-size: var(--font-size-md);
  z-index: 10;
  animation: undo-bar-in 0.2s ease-out;
}

@keyframes undo-bar-in {
  from {
    transform: translateY(100%);
    opacity: 0;
  }
  to {
    transform: translateY(0);
    opacity: 1;
  }
}

.undo-text {
  flex: 1;
  color: var(--color-text-secondary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.move-undo-bar {
  background: var(--color-bg-info-soft);
  border-top: 1px solid var(--color-border-info);
}

.conflict-actions {
  display: flex;
  gap: var(--spacing-sm);
  justify-content: center;
  margin-top: var(--spacing-md);
}

.replace-bar {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
  padding: var(--spacing-sm) var(--spacing-lg);
  border-bottom: 1px solid var(--color-border-subtle);
  background: var(--color-bg-secondary);
}

.replace-input {
  flex: 1;
}

.replace-preview {
  font-size: var(--font-size-md);
  color: var(--color-text-muted);
  white-space: nowrap;
}

.diff-container {
  max-height: 60vh;
  overflow: auto;
  font-family: var(--font-monospace);
  font-size: var(--font-size-md);
  line-height: 1.5;
}

.diff-modal {
  width: 800px;
  max-width: 90vw;
}

.diff-labels {
  display: flex;
  border-bottom: 1px solid var(--color-border);
  background: var(--color-bg-secondary);
  position: sticky;
  top: 0;
  z-index: 1;
}

.diff-label-left,
.diff-label-right {
  flex: 1;
  padding: var(--spacing-xs) var(--spacing-sm);
  font-weight: 600;
  color: var(--color-text-secondary);
  border-right: 1px solid var(--color-border-subtle);
}

.diff-label-right {
  border-right: none;
}

.diff-lines {
  border: 1px solid var(--color-border-subtle);
  border-top: none;
}

.diff-line {
  display: flex;
  min-height: 22px;
}

.diff-line-num {
  width: 44px;
  padding: 0 var(--spacing-xs);
  text-align: right;
  color: var(--color-text-muted);
  background: var(--color-bg-tertiary);
  border-right: 1px solid var(--color-border-subtle);
  user-select: none;
  flex-shrink: 0;
}

.diff-num-right {
  border-right: none;
}

.diff-line-content {
  flex: 1;
  padding: 0 var(--spacing-sm);
  white-space: pre;
  overflow: hidden;
  text-overflow: ellipsis;
}

.diff-unchanged {
  background: transparent;
}

.diff-added {
  background: var(--color-bg-success-soft);
}

.diff-added .diff-line-content::before {
  content: '+ ';
  color: var(--brand-success);
  font-weight: bold;
}

.diff-removed {
  background: var(--color-bg-danger-soft, var(--brand-danger-soft));
}

.diff-removed .diff-line-content::before {
  content: '- ';
  color: var(--brand-danger);
  font-weight: bold;
}

.diff-unchanged .diff-line-content::before {
  content: '  ';
}
</style>
