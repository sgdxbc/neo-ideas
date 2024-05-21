<script setup>
import { onMounted, ref } from "vue"
import PostDetail from "./components/PostDetail.vue"

const categoryId = "default"
const category = ref([])

onMounted(() => {
  console.log("Mounted App");
  (async () => {
    const resp = await fetch(`categories/${categoryId}.json`)
    category.value = await resp.json()
  })()
})
</script>

<template>
  <PostDetail :category path="1716187144792-chopin" />
</template>

<style scoped></style>
