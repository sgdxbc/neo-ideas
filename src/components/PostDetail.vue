<script setup>
import { onMounted, computed, watchEffect } from "vue"
import { DateTime } from "luxon"

const props = defineProps({
  category: {
    type: Array,
    required: true,
  },
  path: {
    type: String,
    required: true,
  }
})

import { ref } from 'vue'

const overview = computed(() => props.category.find((post) => post.path == props.path) ?? null)
const detail = ref({
  document: ["……"]
})

onMounted(() => {
  console.log(`Mounted PostDetail for path ${props.path}`);
  (async () => {
    const resp = await fetch(`posts/${props.path}/current.json`)
    detail.value = await resp.json()
  })()
})

const publishedAt = computed(() => DateTime.fromISO(overview.value.revisions.at(0)))
const lastModifiedAt = computed(() => DateTime.fromISO(overview.value.revisions.at(-1)))

watchEffect(() => {
  if (overview.value && overview.value.title) {
    document.title = overview.value.title + ' | Ideas (neo)'
  }
})
</script>

<template>
  <h1 v-if="overview && Object.hasOwn(overview, 'title')">{{ overview.title }}</h1>
  <div v-if="overview && Object.hasOwn(overview, 'revisions')">
    {{ publishedAt.setLocale('zh').toLocaleString(DateTime.DATETIME_FULL) }}
    <span v-if="overview.revisions.length > 1">
      （最近修订 {{ lastModifiedAt.setLocale('zh').toLocaleString(DateTime.DATETIME_FULL) }}）
    </span>
  </div>
  <hr>
  <div v-for="item in detail.document" :key="item">
    {{ item }}
  </div>
</template>